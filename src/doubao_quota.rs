//! 豆包网页订阅额度。
//!
//! 豆包没有 Codex 那样的本地 JSON token；登录态在 `www.doubao.com` 的 Cookie
//! （`sessionid`、`x-tt-multi-sids`、`msToken`）里。连接方式与 Gemini 一致：
//! 从 Kimi 浏览器扩展读取当前豆包标签页的 Cookie，或由用户手动粘贴。
//! Cookie 用 Windows DPAPI 加密后保存，额度接口是非官方 summary 接口。
use super::{Account, Snapshot, Window};
use serde_json::{json, Value};
use std::{fs, io::Read, path::PathBuf, sync::mpsc};

const ENDPOINT: &str = "https://www.doubao.com/alice/commerce/sale/subscription/quota/summary/";
const LOGIN_ERROR: &str = "豆包登录已失效，请重新连接浏览器账号";
const PARSE_ERROR: &str = "豆包额度接口已变化，暂时无法读取";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Doubao/2.34.6";

fn integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

/// 金额字段可能是数字，也可能是 "2,100" 这样的展示字符串。
fn amount(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.replace(',', "").trim().parse().ok())
        .filter(|number| number.is_finite())
}

fn used_percent(limit: &Value) -> Option<f64> {
    amount(&limit["used_percent"]).or_else(|| {
        let used = amount(&limit["used_amount"])?;
        let total = amount(&limit["total_amount"])?;
        (total > 0.0).then_some(used / total * 100.0)
    })
}

fn window(limit: &Value) -> Option<Window> {
    let seconds = match integer(&limit["window_type"])? {
        1 => Some(18000),
        2 => Some(604800),
        3 => None,
        _ => return None,
    };
    let used = used_percent(limit).unwrap_or_else(|| {
        if limit["usage_exhausted"].as_bool() == Some(true) { 100.0 } else { 0.0 }
    });
    let reset = integer(&limit["end_time"]).filter(|ms| *ms > 0).map(|ms| ms / 1000);
    let label = match seconds {
        Some(18000) => "5 小时",
        Some(604800) => "本周",
        _ => "活动额度",
    };
    Some(Window {
        label: label.into(),
        seconds,
        remaining: (100.0 - used).clamp(0.0, 100.0) as f32,
        reset,
    })
}

pub fn parse(value: &Value) -> Result<Snapshot, String> {
    let data = value.get("data").filter(|data| data.is_object()).ok_or(LOGIN_ERROR)?;
    let mut windows = Vec::new();
    let mut extra = Vec::new();
    for section in ["window_limit_section", "incentive_quota_section"] {
        let groups = data
            .pointer(&format!("/{section}/window_limit_groups"))
            .and_then(Value::as_array);
        for group in groups.into_iter().flatten() {
            for limit in group["window_limits"].as_array().into_iter().flatten() {
                let Some(window) = window(limit) else { continue };
                if integer(&limit["window_type"]) == Some(3) {
                    extra.push(window);
                } else {
                    windows.push(window);
                }
            }
        }
    }
    if windows.is_empty() {
        if extra.is_empty() {
            return Err("豆包账号暂未提供订阅额度".into());
        }
        windows = extra;
        extra = Vec::new();
    }
    windows.sort_by_key(|window| window.seconds.unwrap_or(i64::MAX));
    let plan = data
        .pointer("/current_subscription/display/short_name")
        .or_else(|| data.pointer("/current_subscription/display/product_name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    // 面板会拼成「豆包 · {plan}」，去掉产品名里重复的厂商前缀。
    let plan = plan.strip_prefix("豆包").map(str::trim).unwrap_or(plan).to_owned();
    Ok(Snapshot { plan, windows, extra, at: chrono::Utc::now().timestamp() })
}

fn cookie(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let raw = raw.strip_prefix("Cookie:").or_else(|| raw.strip_prefix("cookie:")).unwrap_or(raw).trim();
    if raw.is_empty() || raw.len() > 32768 || !raw.is_ascii() || raw.bytes().any(|byte| byte < 32 || byte == 127) {
        return Err("请粘贴有效的 Cookie 请求头".into());
    }
    let session = raw
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(name, _)| *name == "sessionid")
        .map(|(_, value)| value)
        .unwrap_or_default();
    if session.len() != 32 || !session.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Cookie 中没有有效的豆包 sessionid，请重新复制".into());
    }
    Ok(raw.into())
}

fn session_value<'a>(cookie: &'a str, name: &str) -> Option<&'a str> {
    cookie.split(';').filter_map(|part| part.trim().split_once('=')).find(|(key, _)| *key == name).map(|(_, value)| value)
}

fn slot(cookie: &str) -> String {
    // sessionid 会轮换；优先用更稳定的 uid_tt，保证重连后仍归到同一个账号。
    let identity = session_value(cookie, "uid_tt")
        .or_else(|| session_value(cookie, "sessionid"))
        .unwrap_or(cookie);
    ring::digest::digest(&ring::digest::SHA256, identity.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn session_path(slot: &str) -> Result<PathBuf, String> {
    if slot.len() != 64 || !slot.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LOGIN_ERROR.into());
    }
    Ok(crate::model::config_dir().join("credentials").join(format!("doubao-{slot}.bin")))
}

fn save(raw_cookie: &str) -> Result<String, String> {
    let slot = slot(raw_cookie);
    let path = session_path(&slot)?;
    let raw = json!({"cookie": raw_cookie}).to_string();
    let encrypted = super::protect_doubao(raw.as_bytes(), true)?;
    fs::create_dir_all(path.parent().ok_or("无法保存豆包登录")?).map_err(|_| "无法保存豆包登录")?;
    crate::model::atomic_write(&path, &encrypted).map_err(|_| "无法保存豆包登录")?;
    Ok(slot)
}

fn load(account: &Account) -> Result<String, String> {
    let expected = account.slot.as_deref().ok_or(LOGIN_ERROR)?;
    let path = session_path(expected)?;
    let encrypted = fs::read(path).map_err(|_| LOGIN_ERROR)?;
    let raw = super::protect_doubao(&encrypted, false)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| LOGIN_ERROR)?;
    let raw_cookie = cookie(value["cookie"].as_str().ok_or(LOGIN_ERROR)?)?;
    if slot(&raw_cookie) != expected {
        return Err("豆包账号不一致，请重新连接".into());
    }
    Ok(raw_cookie)
}

pub fn forget(account: &Account) {
    if let Some(path) = account.slot.as_deref().and_then(|slot| session_path(slot).ok()) {
        let _ = fs::remove_file(path);
    }
}

fn request_url(cookie: &str) -> String {
    let mut url = url::Url::parse(ENDPOINT).unwrap();
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in [
            ("aid", "582478"),
            ("real_aid", "582478"),
            ("version_code", "20800"),
            ("language", "zh"),
            ("device_platform", "web"),
            ("doubao_device_platform", "desktop"),
            ("pkg_type", "release_version"),
            ("device_id", "177462281012986"),
            ("pc_version", "2.34.6"),
            ("doubao_pc_version", "2.34.6"),
            ("web_id", "7662832405736687142"),
            ("tea_uuid", "177462281012986"),
            ("region", "CN"),
            ("sys_region", "CN"),
            ("samantha_web", "1"),
            ("web_platform", "desktop"),
            ("use-olympus-account", "1"),
            ("runtime", "web"),
            ("runtime_version", "3.43.4"),
            ("client_platform", "pc_client"),
            ("chromium_version", "147.0.7727.149"),
            ("channel", "win"),
            ("fp", "verify_177462281012986"),
            ("web_tab_id", "02a97436-86cc-479a-95e7-e9d72a76bcd1"),
        ] {
            query.append_pair(key, value);
        }
        if let Some(ms_token) = session_value(cookie, "msToken") {
            query.append_pair("msToken", ms_token);
        }
    }
    url.into()
}

fn request(agent: &ureq::Agent, cookie: &str) -> Result<Snapshot, (String, u64)> {
    let response = agent
        .post(&request_url(cookie))
        .set("Cookie", cookie)
        .set("Origin", "https://www.doubao.com")
        .set("Referer", "https://www.doubao.com/member/quota-management")
        .set("Accept", "application/json, text/plain, */*")
        .set("User-Agent", USER_AGENT)
        .send_string(r#"{"product_line":"membership"}"#)
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => (LOGIN_ERROR.into(), 0),
            ureq::Error::Status(429, response) => (
                "豆包请求过于频繁，稍后自动重试".into(),
                response.header("Retry-After").and_then(|value| value.parse().ok()).unwrap_or(180).clamp(30, 3600),
            ),
            ureq::Error::Status(code, _) => (format!("豆包额度服务暂不可用（{code}）"), 60),
            _ => ("无法连接豆包额度服务，请检查网络或代理".into(), 30),
        })?;
    let mut body = String::new();
    response.into_reader().take(1024 * 1024).read_to_string(&mut body).map_err(|_| ("读取豆包额度失败".into(), 30))?;
    let value: Value = serde_json::from_str(&body).map_err(|_| (PARSE_ERROR.into(), 60))?;
    if let Some(code) = integer(&value["code"]).filter(|code| *code != 0) {
        return Err((format!("豆包额度服务返回错误（{code}）"), 60));
    }
    parse(&value).map_err(|error| (error, 60))
}

pub fn fetch(agent: &ureq::Agent, account: &Account) -> Result<Snapshot, (String, u64)> {
    let cookie = load(account).map_err(|error| (error, 0))?;
    request(agent, &cookie)
}

pub struct Connected {
    pub slot: String,
    pub label: String,
    pub snapshot: Snapshot,
}

pub struct Connect {
    rx: mpsc::Receiver<Result<Connected, String>>,
}

impl Connect {
    pub fn start(alias: String, header: Option<String>, expected: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let raw_cookie = match header {
                    Some(header) => cookie(&header)?,
                    None => cookie(&super::browser_cookie_header("www.doubao.com", "https://www.doubao.com/chat/", "豆包")?)?,
                };
                if expected.as_deref().is_some_and(|slot| slot != crate::quota::doubao::slot(&raw_cookie)) {
                    return Err("当前浏览器是另一个豆包账号，请切回原账号后重新连接".into());
                }
                let snapshot = request(&super::http_agent(), &raw_cookie).map_err(|error| error.0)?;
                let slot = save(&raw_cookie)?;
                let label = if alias.trim().is_empty() { format!("豆包 · {}", &slot[..6]) } else { alias.trim().to_owned() };
                Ok(Connected { slot, label, snapshot })
            })();
            let _ = tx.send(result);
        });
        Self { rx }
    }

    pub fn poll(&self) -> Option<Result<Connected, String>> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> Value {
        json!({"code":0,"data":{
            "current_subscription":{"display":{"short_name":"豆包 Pro"}},
            "window_limit_section":{"window_limit_groups":[{"window_limits":[
                {"window_type":1,"used_percent":28,"start_time":1899990000000i64,"end_time":1900008000000i64},
                {"window_type":2,"used_percent":56,"start_time":1899400000000i64,"end_time":1900604800000i64}
            ]}]},
            "incentive_quota_section":{"window_limit_groups":[{"window_limits":[
                {"window_type":3,"total_amount":"1,000","used_amount":"250","end_time":1900000000000i64}
            ]}]}
        }})
    }

    #[test]
    fn parses_five_hour_week_and_bonus_windows() {
        let snapshot = parse(&response()).unwrap();
        assert_eq!(snapshot.plan, "Pro");
        assert_eq!(snapshot.rings().len(), 2);
        assert_eq!(snapshot.rings()[0].label, "5 小时");
        assert_eq!(snapshot.rings()[0].remaining, 72.0);
        assert_eq!(snapshot.rings()[1].label, "本周");
        assert_eq!(snapshot.rings()[1].remaining, 44.0);
        assert_eq!(snapshot.rings()[0].reset, Some(1900008000));
        assert_eq!(snapshot.extra.len(), 1);
        assert_eq!(snapshot.extra[0].remaining, 75.0);
    }

    #[test]
    fn amount_fallback_and_zero_timestamps_stay_honest() {
        let value = json!({"data":{"window_limit_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":1,"total_amount":"3,000","used_amount":"2,100","start_time":0,"end_time":0}
        ]}]}}});
        let snapshot = parse(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].remaining, 30.0);
        assert_eq!(snapshot.windows[0].reset, None, "服务端没给重置时间时不能编一个");
    }

    #[test]
    fn missing_data_or_windows_is_an_error() {
        assert_eq!(parse(&json!({})).unwrap_err(), LOGIN_ERROR);
        assert!(parse(&json!({"data":{}})).is_err());
        assert!(parse(&json!({"data":{"window_limit_section":{"window_limit_groups":[]}}})).is_err());
    }

    #[test]
    fn cookie_requires_a_real_sessionid() {
        let value = format!("sessionid={}; x-tt-multi-sids=123%3Aabcdef", "a".repeat(32));
        assert_eq!(cookie(&format!("Cookie: {value}")).unwrap(), value);
        for bad in ["", "foo=bar", "sessionid=short", "sessionid=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nX: y"] {
            assert!(cookie(bad).is_err(), "{bad:?}");
        }
        assert_ne!(slot(&value), slot(&format!("sessionid={}", "b".repeat(32))));
        assert_eq!(slot(&value), slot(&format!("sessionid={}; other=1", "a".repeat(32))));
        // sessionid 轮换但 uid_tt 不变时，仍应视为同一个账号。
        let rotated = |session: &str| format!("sessionid={session}; uid_tt=user-1");
        assert_eq!(slot(&rotated(&"a".repeat(32))), slot(&rotated(&"b".repeat(32))));
    }

    #[test]
    fn request_url_carries_client_identity_and_ms_token() {
        let value = format!("sessionid={}; msToken=token-value", "a".repeat(32));
        let url = url::Url::parse(&request_url(&value)).unwrap();
        let query = url.query_pairs().collect::<std::collections::HashMap<_, _>>();
        assert_eq!(query["aid"], "582478");
        assert_eq!(query["pc_version"], "2.34.6");
        assert_eq!(query["msToken"], "token-value");
    }

    #[cfg(windows)]
    #[test]
    fn windows_storage_is_encrypted_and_roundtrips() {
        let raw = br#"{"cookie":"sessionid=synthetic-secret"}"#;
        let sealed = super::super::protect_doubao(raw, true).unwrap();
        assert!(!sealed.windows(16).any(|window| window == b"synthetic-secret"));
        assert_eq!(super::super::protect_doubao(&sealed, false).unwrap(), raw);
        let mut corrupt = sealed;
        corrupt[24] ^= 1;
        assert!(super::super::protect_doubao(&corrupt, false).is_err());
    }
}

//! 刘海面板轮廓：顶部向外翻的「肩」+ 竖直侧边 + 底部大圆角。
//!
//! 轮廓左右对称、自上而下单调，所以按行切成梯形条填充（可处理凹肩），
//! 再沿外轮廓补一圈 1 物理像素的羽化带做抗锯齿——边缘不再有锯齿台阶。

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// (y, 左 x, 右 x) 轮廓采样行。
pub fn sections(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<(f32, f32, f32)> {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let r = radius.min(rect.height() * 0.5).min(rect.width() * 0.5).max(0.0);
    let fh = flare_h.min(rect.height() * 0.5).max(0.5);
    let mut rows = Vec::with_capacity(48);
    const N_FLARE: usize = 14;
    for i in 0..=N_FLARE {
        let t = i as f32 / N_FLARE as f32;
        // 肩部曲线：(1-t)^2.4 比三次方更饱满，过渡更柔。
        let wing = flare * (1.0 - t).powf(2.4);
        rows.push((y0 + fh * t, x0 - wing, x1 + wing));
    }
    if y1 - r > y0 + fh {
        rows.push((y1 - r, x0, x1));
    }
    const N_CORNER: usize = 20;
    for i in 1..=N_CORNER {
        let a = i as f32 / N_CORNER as f32 * std::f32::consts::FRAC_PI_2;
        rows.push((y1 - r + r * a.sin(), x0 + r - r * a.cos(), x1 - r + r * a.cos()));
    }
    rows
}

/// 闭合外轮廓（顺时针：右侧自上而下，再左侧自下而上）。
pub fn outline(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<Pos2> {
    let rows = sections(rect, flare, flare_h, radius);
    rows.iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect()
}

// 顶边不参与羽化；端点只取肩部切线，避免整条顶边扭曲法线。
fn edge_normal(pts: &[Pos2], i: usize) -> Vec2 {
    let normal = |a: Pos2, b: Pos2| {
        let d = (b - a).normalized();
        Vec2::new(d.y, -d.x)
    };
    if i == 0 {
        return normal(pts[0], pts[1]);
    }
    if i == pts.len() - 1 {
        return normal(pts[i - 1], pts[i]);
    }
    let prev = normal(pts[i - 1], pts[i]);
    let next = normal(pts[i], pts[i + 1]);
    // 相邻单位法线的交点补偿，保持两段的垂直羽化宽度一致。
    (prev + next) / (1.0 + prev.dot(next))
}

/// 抗锯齿填充。`feather` 是羽化宽度（逻辑点，通常 = 1 / ppp）。
pub fn fill(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, color: Color32, feather: f32) {
    fill_gradient(painter, rect, flare, flare_h, radius, color, color, feather);
}

/// 上下渐变的抗锯齿填充；玻璃顶部柔光用。
pub fn fill_gradient(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, top: Color32, bottom: Color32, feather: f32) {
    let key=([rect.min.x,rect.min.y,rect.max.x,rect.max.y,flare,flare_h,radius,feather].map(f32::to_bits),top,bottom);
    let mesh=painter.ctx().memory_mut(|memory|memory.caches.cache::<FillCache>().get(key));
    painter.add(egui::Shape::Mesh(mesh));
}

type FillKey=([u32;8],Color32,Color32);
type FillCache=egui::cache::FrameCache<std::sync::Arc<egui::Mesh>,FillComputer>;
#[derive(Default)]
struct FillComputer;
impl egui::cache::ComputerMut<FillKey,std::sync::Arc<egui::Mesh>> for FillComputer {
    fn compute(&mut self,(bits,top,bottom):FillKey)->std::sync::Arc<egui::Mesh>{
        let [x0,y0,x1,y1,flare,flare_h,radius,feather]=bits.map(f32::from_bits);
        std::sync::Arc::new(fill_mesh_gradient(Rect::from_min_max(Pos2::new(x0,y0),Pos2::new(x1,y1)),flare,flare_h,radius,top,bottom,feather))
    }
}

fn mix_color(a:Color32,b:Color32,t:f32)->Color32{
    let t=t.clamp(0.0,1.0);
    let mix=|a:u8,b:u8|(a as f32+(b as f32-a as f32)*t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(),b.r()),mix(a.g(),b.g()),mix(a.b(),b.b()),mix(a.a(),b.a()))
}

// 仅保留当前帧仍使用的轮廓；尺寸、透明度或 DPI 改变时重新生成，动画不复用旧形状。
#[cfg(test)]
fn fill_mesh(rect:Rect,flare:f32,flare_h:f32,radius:f32,color:Color32,feather:f32)->egui::Mesh {
    fill_mesh_gradient(rect,flare,flare_h,radius,color,color,feather)
}

fn fill_mesh_gradient(rect:Rect,flare:f32,flare_h:f32,radius:f32,top:Color32,bottom:Color32,feather:f32)->egui::Mesh {
    let rows = sections(rect, flare, flare_h, radius);
    let height=rect.height().max(1.0);
    let at=|y:f32|mix_color(top,bottom,(y-rect.top())/height);
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve((rows.len()*3-2)*4);
    mesh.indices.reserve((rows.len()*3-2)*6);
    // 1) 实心梯形条
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        let (ca,cb)=(at(ya),at(yb));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(la, ya), ca);
        mesh.colored_vertex(Pos2::new(ra, ya), ca);
        mesh.colored_vertex(Pos2::new(rb, yb), cb);
        mesh.colored_vertex(Pos2::new(lb, yb), cb);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    // 2) 外轮廓羽化带（不含顶边：贴着屏幕顶）
    let pts: Vec<Pos2> = rows
        .iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect();
    let n = pts.len();
    let clear = Color32::TRANSPARENT;
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        // 顶边两端（右上角 → 左上角那条闭合边）不羽化
        if a.y <= rect.top() + 0.01 && b.y <= rect.top() + 0.01 {
            continue;
        }
        let (na, nb) = (edge_normal(&pts, i), edge_normal(&pts, i + 1));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, at(a.y));
        mesh.colored_vertex(b, at(b.y));
        mesh.colored_vertex(b + nb * feather, clear);
        mesh.colored_vertex(a + na * feather, clear);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

/// 沿轮廓向内的边缘光/内发光。`width` 是向内宽度，`fade_to` 是底边保留的强度。
pub fn rim(painter:&egui::Painter,rect:Rect,flare:f32,flare_h:f32,radius:f32,color:Color32,width:f32,fade_to:f32){
    if width<=0.0 || color.a()==0 {return;}
    let mesh=rim_mesh(rect,flare,flare_h,radius,color,width,fade_to);
    painter.add(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
}

fn rim_mesh(rect:Rect,flare:f32,flare_h:f32,radius:f32,color:Color32,width:f32,fade_to:f32)->egui::Mesh{
    let pts=outline(rect,flare,flare_h,radius);
    let n=pts.len();
    let height=rect.height().max(1.0);
    let fade=|p:Pos2|{
        let t=((p.y-rect.top())/height).clamp(0.0,1.0);
        let k=1.0+(fade_to.clamp(0.0,1.0)-1.0)*t;
        Color32::from_rgba_premultiplied(
            (color.r() as f32*k).round() as u8,
            (color.g() as f32*k).round() as u8,
            (color.b() as f32*k).round() as u8,
            (color.a() as f32*k).round() as u8,
        )
    };
    let mut mesh=egui::Mesh::default();
    mesh.vertices.reserve(n*4);
    mesh.indices.reserve(n*6);
    for i in 0..n-1{
        let (a,b)=(pts[i],pts[i+1]);
        if a.y<=rect.top()+0.01 && b.y<=rect.top()+0.01 {continue;}
        let (na,nb)=(edge_normal(&pts,i),edge_normal(&pts,i+1));
        let base=mesh.vertices.len() as u32;
        mesh.colored_vertex(a,fade(a));
        mesh.colored_vertex(b,fade(b));
        let inner_b=Pos2::new(b.x-nb.x*width,(b.y-nb.y*width).clamp(rect.top(),rect.bottom()));
        let inner_a=Pos2::new(a.x-na.x*width,(a.y-na.y*width).clamp(rect.top(),rect.bottom()));
        mesh.colored_vertex(inner_b,Color32::TRANSPARENT);
        mesh.colored_vertex(inner_a,Color32::TRANSPARENT);
        mesh.add_triangle(base,base+1,base+2);
        mesh.add_triangle(base,base+2,base+3);
    }
    mesh
}

/// 柔和的落影：多层半透明轮廓叠加，近似高斯模糊（不依赖 GPU 模糊）。
pub fn shadow(painter: &egui::Painter, rect: Rect, flare: f32, flare_h: f32, radius: f32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    for (grow, a) in [(10.0f32, 0.035f32), (6.5, 0.05), (3.5, 0.07), (1.5, 0.09)] {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() - grow * 0.6, rect.top()),
            Pos2::new(rect.right() + grow * 0.6, rect.bottom() + grow),
        );
        let c = Color32::from_black_alpha((255.0 * a * strength) as u8);
        fill(painter, r, flare, flare_h, radius + grow * 0.8, c, grow * 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(points: &[Pos2], p: Pos2) -> bool {
        let mut inside = false;
        let mut prev = *points.last().unwrap();
        for &next in points {
            if (next.y > p.y) != (prev.y > p.y)
                && p.x < (prev.x - next.x) * (p.y - next.y) / (prev.y - next.y) + next.x
            {
                inside = !inside;
            }
            prev = next;
        }
        inside
    }

    #[test]
    fn shoulder_feather_keeps_one_pixel_perpendicular_to_edge() {
        for (width, height, flare, flare_h, radius) in [
            (143.0, 25.0, 5.0, 8.0, 12.5),
            (419.0, 140.0, 17.6, 16.0, 33.0),
        ] {
            let pts = outline(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)), flare, flare_h, radius);
            for ppp in [1.0, 1.25, 1.5, 2.0] {
                for i in 0..pts.len() - 1 {
                    let d = (pts[i + 1] - pts[i]).normalized();
                    let normal = Vec2::new(d.y, -d.x);
                    for j in [i, i + 1] {
                        let offset = edge_normal(&pts, j) / ppp;
                        assert!((offset.dot(normal) * ppp - 1.0).abs() < 0.001,
                            "羽化必须沿每段轮廓保持一物理像素：边 {i}，顶点 {j}，缩放 {ppp}");
                    }
                }
            }
        }
    }

    #[test]
    fn wings_extend_outside_body_only_at_top() {
        let rect = Rect::from_min_size(Pos2::new(110.0, 0.0), Vec2::new(419.0, 140.0));
        let pts = outline(rect, 17.6, 16.0, 33.0);
        assert!(pts.iter().any(|p| p.x < rect.left() - 17.0));
        assert!(inside(&pts, Pos2::new(rect.left() + 0.2, 70.0)));
        assert!(!inside(&pts, Pos2::new(rect.left() - 0.5, 70.0)));
        // 底部圆角外侧是空的
        assert!(!inside(&pts, Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0)));
    }

    #[test]
    fn gradient_fill_runs_from_top_color_to_bottom_color() {
        let rect=Rect::from_min_size(Pos2::ZERO,Vec2::new(419.0,140.0));
        let mesh=fill_mesh_gradient(rect,17.6,16.0,33.0,Color32::WHITE,Color32::BLACK,1.0);
        let top=mesh.vertices.iter().filter(|v|v.pos.y<14.0).map(|v|v.color.r()).max().unwrap();
        let bottom=mesh.vertices.iter().filter(|v|v.pos.y>126.0).map(|v|v.color.r()).min().unwrap();
        assert!(top>bottom,"顶部柔光必须向底部渐暗：{top} <= {bottom}");
    }

    #[test]
    fn rim_band_stays_on_the_outline_and_fades_inward() {
        let rect=Rect::from_min_size(Pos2::ZERO,Vec2::new(419.0,140.0));
        let mesh=rim_mesh(rect,17.6,16.0,33.0,Color32::from_white_alpha(160),2.0,0.3);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len()%4,0);
        assert!(mesh.vertices.iter().all(|v|v.pos.y>=rect.top()-0.01&&v.pos.y<=rect.bottom()+0.01));
        assert!(mesh.vertices.iter().any(|v|v.color.a()>0),"rim 外沿应有颜色");
        assert!(mesh.vertices.iter().any(|v|v.color.a()==0),"rim 内沿应淡出");
        let top_alpha=mesh.vertices.iter().filter(|v|v.pos.y<8.0).map(|v|v.color.a()).max().unwrap_or(0);
        let bottom_alpha=mesh.vertices.iter().filter(|v|v.pos.y>132.0).map(|v|v.color.a()).max().unwrap_or(255);
        assert!(top_alpha>bottom_alpha,"底边 rim 应比顶边弱");
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    fn frame(ctx:&egui::Context,width:f32,feather:f32)->std::sync::Arc<egui::Mesh>{
        let out=ctx.run(egui::RawInput::default(),|ctx|{
            fill(&ctx.layer_painter(egui::LayerId::background()),Rect::from_min_size(Pos2::ZERO,Vec2::new(width,30.0)),5.0,8.0,12.5,Color32::BLACK,feather);
        });
        out.shapes.into_iter().find_map(|s|if let egui::Shape::Mesh(m)=s.shape{Some(m)}else{None}).unwrap()
    }
    #[test]
    fn unchanged_outline_reuses_geometry_across_frames(){
        let ctx=egui::Context::default();let first=frame(&ctx,300.0,1.0);let second=frame(&ctx,300.0,1.0);
        assert!(std::sync::Arc::ptr_eq(&first,&second),"稳定轮廓不能每帧重新分配网格");
        let changed=frame(&ctx,320.0,1.0);assert_ne!(first.vertices,changed.vertices);
        let scaled=frame(&ctx,320.0,0.5);assert_ne!(changed.vertices,scaled.vertices);
    }

    #[test]
    fn animated_outlines_do_not_accumulate_old_meshes(){
        let ctx=egui::Context::default();
        for i in 0..300 {
            let mesh=frame(&ctx,300.0+i as f32/10.0,1.0);
            let expected=fill_mesh(Rect::from_min_size(Pos2::ZERO,Vec2::new(300.0+i as f32/10.0,30.0)),5.0,8.0,12.5,Color32::BLACK,1.0);
            assert_eq!(mesh.vertices,expected.vertices);assert_eq!(mesh.indices,expected.indices);
            let count=ctx.memory_mut(|m|egui::cache::CacheTrait::len(m.caches.cache::<FillCache>()));
            assert!(count<=2,"动画不能累积历史轮廓：{count}");
        }
    }
}

//! SVG export of the epicycle machinery + reconstructed trace.
//! Format style mirrors fourier-svg-rs / fluffy-eureka HTML visualizations:
//! one <g> per contour, circles for the chain, polyline for the trace.

use crate::fourier::ContourTransform;
use crate::render::RenderCfg;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn to_svg(cfg: &RenderCfg, trs: &[ContourTransform], title: &str) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        cfg.width, cfg.height, cfg.width, cfg.height
    ));
    s.push_str(&format!("<title>{}</title>\n", esc(title)));
    s.push_str(&format!(
        "<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n"
    ));

    for tr in trs {
        s.push_str(&format!("<g id=\"contour-{}\">\n", tr.id));
        // Epicycle chain at t = 0.
        let chain = tr.evaluate_chain(0.0);
        for (i, pair) in chain.windows(2).enumerate() {
            let a = pair[0];
            let b = pair[1];
            let r = tr.harmonics[i].mag() * cfg.scale;
            if r >= 0.5 {
                s.push_str(&format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"none\" stroke=\"#7882a5\" stroke-width=\"0.8\"/>\n",
                    a.x * cfg.scale,
                    a.y * cfg.scale,
                    r
                ));
            }
            s.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#c8cddc\" stroke-width=\"1\"/>\n",
                a.x * cfg.scale,
                a.y * cfg.scale,
                b.x * cfg.scale,
                b.y * cfg.scale
            ));
        }
        // Trace polyline: step count adapts to the perimeter so segments
        // stay visible on small contours (like render::draw_reconstruction).
        let steps = (tr.perimeter_px * cfg.scale / 0.4).round().clamp(64.0, 720.0) as usize;
        let mut pts = String::new();
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let p = tr.evaluate(t);
            pts.push_str(&format!("{:.2},{:.2} ", p.x * cfg.scale, p.y * cfg.scale));
        }
        s.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"#d22828\" stroke-width=\"1.6\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n",
            pts.trim()
        ));
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}
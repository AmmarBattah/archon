//! `report.html`: the summary as a self-contained page (inline SVG, no
//! scripts or network), optionally opened for review with `lavish-axi`.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::summary::{Aggregate, Stat, Summary};

const BAR_WIDTH: f64 = 420.0;
const ROW_HEIGHT: f64 = 26.0;
const LABEL_WIDTH: f64 = 80.0;
/// One colour per arm, in `Arm::ALL` order, distinguishable in greyscale.
const ARM_COLOURS: [&str; 4] = ["#1f5fa8", "#d9822b", "#5a9e4b", "#8a5fb0"];

/// One chart: a metric per arm, with the stdev drawn as a whisker.
struct Metric {
    title: &'static str,
    unit: &'static str,
    value: fn(&Aggregate) -> Option<Stat>,
}

const METRICS: [Metric; 5] = [
    Metric {
        title: "Success rate",
        unit: "%",
        value: |a| {
            Some(Stat {
                n: a.trials as usize,
                mean: a.success_rate * 100.0,
                stdev: 0.0,
            })
        },
    },
    Metric {
        title: "Wall time",
        unit: "s",
        value: |a| Some(a.wall_secs),
    },
    Metric {
        title: "Cost",
        unit: "$",
        value: |a| a.cost_usd,
    },
    Metric {
        title: "Total tokens (input + output)",
        unit: "tok",
        value: |a| {
            let (input, output) = (a.input_tokens?, a.output_tokens?);
            Some(Stat {
                n: input.n,
                mean: input.mean + output.mean,
                stdev: (input.stdev.powi(2) + output.stdev.powi(2)).sqrt(),
            })
        },
    },
    Metric {
        title: "Operator-visible messages",
        unit: "msgs",
        value: |a| Some(a.operator_messages),
    },
];

pub fn write_report(dir: &Path, summary: &Summary, markdown: &str) -> anyhow::Result<PathBuf> {
    let path = dir.join("report.html");
    std::fs::write(&path, render_html(summary, markdown))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

pub fn render_html(summary: &Summary, markdown: &str) -> String {
    let mut out = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Archon benchmark</title>\
         <style>body{font:15px/1.45 system-ui,sans-serif;max-width:1100px;margin:2em auto;padding:0 1em;color:#222}\
         .charts{display:grid;grid-template-columns:repeat(auto-fit,minmax(520px,1fr));gap:1.5em}\
         figure{margin:0}figcaption{font-weight:600;margin-bottom:.3em}svg text{font-size:12px}\
         pre{white-space:pre-wrap;background:#f6f6f6;padding:1em;border-radius:4px;font-size:13px}</style></head><body>\
         <h1>Archon benchmark</h1>",
    );
    if let Some(m) = &summary.manifest {
        out.push_str(&format!(
            "<p>Run <code>{}</code>, effort <code>{}</code>, {} trials per task and arm.</p>",
            escape(&m.stamp),
            escape(&m.effort),
            m.trials
        ));
    }
    out.push_str("<div class=\"charts\">");
    for metric in &METRICS {
        out.push_str(&bar_chart(metric, &summary.arms));
    }
    out.push_str("</div><h2>Full summary</h2><pre>");
    out.push_str(&escape(markdown));
    out.push_str("</pre></body></html>\n");
    out
}

fn bar_chart(metric: &Metric, arms: &[Aggregate]) -> String {
    let rows: Vec<(&Aggregate, Stat)> = arms
        .iter()
        .filter_map(|a| Some((a, (metric.value)(a)?)))
        .collect();
    let max = rows
        .iter()
        .map(|(_, s)| s.mean + s.stdev)
        .fold(0.0_f64, f64::max);
    let scale = if max > 0.0 { BAR_WIDTH / max } else { 0.0 };
    let height = ROW_HEIGHT * rows.len().max(1) as f64 + 8.0;
    let width = LABEL_WIDTH + BAR_WIDTH + 130.0;
    let mut svg = format!(
        "<figure><figcaption>{} ({})</figcaption><svg role=\"img\" aria-label=\"{}\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">",
        metric.title, metric.unit, metric.title
    );
    for (row, (aggregate, stat)) in rows.iter().enumerate() {
        let y = ROW_HEIGHT * row as f64 + 4.0;
        let colour = ARM_COLOURS[aggregate.arm as usize % ARM_COLOURS.len()];
        let bar = stat.mean * scale;
        svg.push_str(&format!(
            "<text x=\"0\" y=\"{}\">{}</text><rect x=\"{LABEL_WIDTH}\" y=\"{y}\" width=\"{bar:.1}\" height=\"{}\" fill=\"{colour}\"><title>{}</title></rect>",
            y + 14.0, aggregate.arm, ROW_HEIGHT - 8.0, escape(&aggregate.model)
        ));
        if stat.stdev > 0.0 {
            let (lo, hi) = (
                (stat.mean - stat.stdev).max(0.0) * scale,
                (stat.mean + stat.stdev) * scale,
            );
            let mid = y + (ROW_HEIGHT - 8.0) / 2.0;
            svg.push_str(&format!(
                "<line x1=\"{:.1}\" x2=\"{:.1}\" y1=\"{mid}\" y2=\"{mid}\" stroke=\"#222\"/>",
                LABEL_WIDTH + lo,
                LABEL_WIDTH + hi
            ));
        }
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{}\">{}</text>",
            LABEL_WIDTH + bar.max((stat.mean + stat.stdev) * scale) + 6.0,
            y + 14.0,
            value_label(stat)
        ));
    }
    svg.push_str("</svg></figure>");
    svg
}

fn value_label(stat: &Stat) -> String {
    let digits = if stat.mean.abs() >= 100.0 { 0 } else { 2 };
    if stat.stdev > 0.0 {
        format!(
            "{:.*} ± {:.*} (n={})",
            digits, stat.mean, digits, stat.stdev, stat.n
        )
    } else {
        format!("{:.*} (n={})", digits, stat.mean, stat.n)
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Open the report in Lavish for review; the session URL is printed by
/// `lavish-axi` itself.
pub fn open_in_lavish(report: &Path) -> anyhow::Result<()> {
    assert!(report.is_file(), "{} does not exist", report.display());
    let status = std::process::Command::new("lavish-axi")
        .arg(report)
        .status()
        .context("running lavish-axi (is it installed?)")?;
    anyhow::ensure!(status.success(), "lavish-axi exited with {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Arm;
    use crate::metrics::tests::result;

    #[test]
    fn report_has_one_chart_per_metric_and_escapes_text() {
        let mut claude = result(Some(1.0), None);
        claude.input_tokens = Some(10);
        claude.output_tokens = Some(5);
        let mut archon = result(Some(2.0), None);
        archon.arm = Arm::Archon;
        archon.model = "a<b>".into();
        let summary = Summary::build(&[claude, archon], None).unwrap();
        let html = render_html(&summary, "| x <y> |");
        assert_eq!(html.matches("<svg").count(), METRICS.len());
        assert!(html.contains("a&lt;b&gt;") && html.contains("| x &lt;y&gt; |"));
        assert!(!html.contains("<script"), "the report is static");
        let tokens = bar_chart(&METRICS[3], &summary.arms);
        assert_eq!(
            tokens.matches("<rect").count(),
            1,
            "archon had no token counts"
        );
    }
}

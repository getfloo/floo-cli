use std::collections::HashMap;
use std::process;

use crate::api_types::{AppAnalyticsResponse, TimeSeriesPoint};
use crate::errors::ErrorCode;
use crate::output;

pub fn analytics(app: Option<String>, period: &str) {
    super::require_auth();
    let client = super::init_client(None);

    match app {
        Some(app_name) => app_analytics(&client, &app_name, period),
        None => org_analytics(&client, period),
    }
}

fn app_analytics(client: &crate::api_client::FlooClient, app_name: &str, period: &str) {
    let app_data = super::resolve_app_or_exit(client, app_name);

    let app_id = &app_data.id;
    let name = &app_data.name;

    let data = match client.get_app_analytics(app_id, period) {
        Ok(d) => d,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success(
            &format!("Analytics for {name}"),
            Some(output::to_value(&data)),
        );
        return;
    }

    render_app_analytics(name, period, &data);
}

fn org_analytics(client: &crate::api_client::FlooClient, period: &str) {
    let data = match client.get_org_analytics(period) {
        Ok(d) => d,
        Err(e) => {
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    if output::is_json_mode() {
        output::success("Organization analytics", Some(output::to_value(&data)));
        return;
    }

    render_org_analytics(period, &data);
}

fn render_app_analytics(name: &str, period: &str, data: &AppAnalyticsResponse) {
    let summary = &data.summary;
    let total_requests = summary.total_requests;
    let total_errors = summary.total_errors;
    let error_rate = summary.error_rate;

    eprintln!();
    eprintln!("Analytics for {} (last {})", name, format_period(period));
    eprintln!();
    eprintln!("  {:14}{:>10}", "Requests", format_number(total_requests));
    eprintln!(
        "  {:14}{:>10} ({:.2}%)",
        "Errors",
        format_number(total_errors),
        error_rate * 100.0
    );

    if let Some(avg) = summary.avg_latency_ms {
        eprintln!("  {:14}{:>10}", "Avg Latency", format!("{}ms", avg));
    }
    if let Some(p95) = summary.p95_latency_ms {
        eprintln!("  {:14}{:>10}", "P95 Latency", format!("{}ms", p95));
    }
    if let Some(unique) = summary.unique_users {
        eprintln!("  {:14}{:>10}", "Unique Users", format_number(unique));
    }

    if let Some(breakdown) = &summary.status_code_breakdown {
        if !breakdown.is_empty() {
            eprintln!();
            eprintln!("Status Codes");
            render_status_code_chart(breakdown, total_requests);
        }
    }

    if let Some(time_series) = &data.time_series {
        if !time_series.is_empty() {
            eprintln!();
            eprintln!("Traffic");
            render_sparkline(time_series);
        }
    }

    eprintln!();
}

fn render_org_analytics(period: &str, data: &AppAnalyticsResponse) {
    let summary = &data.summary;
    let total_requests = summary.total_requests;
    let total_errors = summary.total_errors;
    let error_rate = summary.error_rate;
    let apps_with_traffic = summary.total_apps_with_traffic.unwrap_or(0);

    eprintln!();
    eprintln!("Organization Analytics (last {})", format_period(period));
    eprintln!();
    eprintln!(
        "  {:18}{:>10}",
        "Total Requests",
        format_number(total_requests)
    );
    eprintln!(
        "  {:18}{:>10} ({:.2}%)",
        "Total Errors",
        format_number(total_errors),
        error_rate * 100.0
    );
    eprintln!(
        "  {:18}{:>10}",
        "Apps w/ Traffic",
        format_number(apps_with_traffic)
    );

    if let Some(apps) = &data.apps {
        if !apps.is_empty() {
            eprintln!();

            let rows: Vec<Vec<String>> = apps
                .iter()
                .map(|a| {
                    vec![
                        a.app_name.clone(),
                        format_number(a.total_requests),
                        format_number(a.total_errors),
                        format!("{:.2}%", a.error_rate * 100.0),
                    ]
                })
                .collect();

            output::table(&["App", "Requests", "Errors", "Error Rate"], &rows, None);
        }
    }

    if let Some(time_series) = &data.time_series {
        if !time_series.is_empty() {
            eprintln!();
            eprintln!("Traffic");
            render_sparkline(time_series);
        }
    }

    eprintln!();
}

fn render_status_code_chart(breakdown: &HashMap<String, i64>, total: i64) {
    let max_bar_width: usize = 20;

    let mut entries: Vec<(&str, i64)> = breakdown
        .iter()
        .map(|(k, &count)| (k.as_str(), count))
        .collect();
    entries.sort_by_key(|(k, _)| k.to_string());

    let max_count = entries
        .iter()
        .map(|(_, c)| *c)
        .max()
        .expect("entries is non-empty: caller checks breakdown.is_empty()");

    for (bucket, count) in &entries {
        let bar_len = if max_count > 0 {
            (*count as usize * max_bar_width) / max_count as usize
        } else {
            0
        };
        let bar_len = bar_len.max(1);
        let bar: String = "\u{2588}".repeat(bar_len);
        let pct = if total > 0 {
            *count as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<4} {:20} {:>6} ({:.1}%)",
            bucket,
            bar,
            format_number(*count),
            pct
        );
    }
}

fn render_sparkline(time_series: &[TimeSeriesPoint]) {
    let blocks = [
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];

    let counts: Vec<i64> = time_series.iter().map(|p| p.request_count).collect();

    let max = counts
        .iter()
        .copied()
        .max()
        .expect("counts is non-empty: caller checks time_series.is_empty()")
        .max(1);
    let min = counts
        .iter()
        .copied()
        .min()
        .expect("counts is non-empty: caller checks time_series.is_empty()");
    let range = (max - min).max(1);

    let spark: String = counts
        .iter()
        .map(|&c| {
            let idx = ((c - min).saturating_mul(7) / range) as usize;
            blocks[idx.min(7)]
        })
        .collect();

    eprintln!("  {spark}");
}

fn format_number(n: i64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

fn format_period(period: &str) -> &str {
    match period {
        "7d" => "7 days",
        "30d" => "30 days",
        "90d" => "90 days",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_no_commas() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_with_commas() {
        assert_eq!(format_number(1_000), "1,000");
        assert_eq!(format_number(12_450), "12,450");
        assert_eq!(format_number(1_234_567), "1,234,567");
    }

    #[test]
    fn format_period_known() {
        assert_eq!(format_period("7d"), "7 days");
        assert_eq!(format_period("30d"), "30 days");
        assert_eq!(format_period("90d"), "90 days");
    }

    #[test]
    fn format_period_unknown() {
        assert_eq!(format_period("1y"), "1y");
    }
}

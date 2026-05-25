use plotters::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let categories: &[&str] = &["Nonce Prep", "Curve Arith", "Total Commit", "Total Verify"];
    let baseline_data = vec![0.0870, 127.7490, 93.9990, 153.0130];
    let ours_data     = vec![0.5290, 122.9950, 110.7540, 146.2340];

    let root = BitMapBackend::new("benchmark_results.png", (800, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            "Pedersen Commitment Performance (Baseline vs Identity-Binding)",
            ("sans-serif", 30).into_font(),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(
            categories.into_segmented(),   // ValueType becomes SegmentValue<&&str>
            0f64..180f64,
        )?;

    chart.configure_mesh().y_desc("Time (microseconds)").draw()?;

    // Draw Baseline bars — pass &&str by wrapping each &str in a reference
    chart
        .draw_series(
            Histogram::vertical(&chart)
                .style(BLUE.mix(0.6).filled())
                .data(
                    baseline_data
                        .iter()
                        .zip(categories.iter())   // yields (&f64, &&str)
                        .map(|(y, x)| (x, *y)),   // x is &&str — matches SegmentValue<&&str>
                ),
        )?
        .label("Baseline (Standard)")
        .legend(|(x, y)| {
            Rectangle::new([(x, y - 5), (x + 10, y + 5)], BLUE.mix(0.6).filled())
        });

    // Draw Ours bars
    chart
        .draw_series(
            Histogram::vertical(&chart)
                .style(RED.mix(0.6).filled())
                .data(
                    ours_data
                        .iter()
                        .zip(categories.iter())   // yields (&f64, &&str)
                        .map(|(y, x)| (x, *y)),   // x is &&str
                ),
        )?
        .label("Identity-Binding (Ours)")
        .legend(|(x, y)| {
            Rectangle::new([(x, y - 5), (x + 10, y + 5)], RED.mix(0.6).filled())
        });

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .draw()?;

    println!("✅ Graph generated: benchmark_results.png");
    Ok(())
}
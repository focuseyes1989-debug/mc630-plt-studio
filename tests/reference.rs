use mc630_plt_studio::{Settings, export, plt, svg};
#[test]
fn reference_plot_preserves_geometry() {
    let original = plt(
        "TB25,2200,1600;CT1;PA;PU0,0;PU400,400;PD400,400;PD1200,400;PD1200,1000;PD400,400;PU1600,500;PD2000,500;PD2000,1200;PD1600,1200;PD1600,500;PU0,0;!PG;",
        40.,
    ).unwrap();
    let b = original.bounds();
    let settings = Settings {
        width: b[2] - b[0],
        height: b[3] - b[1],
        units: 40.,
        mirror: false,
        rotate: false,
    };
    let generated = export(&original, &settings).unwrap();
    let roundtrip = plt(&generated, 40.).unwrap();
    assert_eq!(original.paths.len(), roundtrip.paths.len());
    for (a, bpath) in original.paths.iter().zip(&roundtrip.paths) {
        let mut normalized = a
            .iter()
            .map(|p| [p[0] - b[0], p[1] - b[1]])
            .collect::<Vec<_>>();
        normalized.dedup();
        assert_eq!(normalized.len(), bpath.len());
        for (p, q) in normalized.iter().zip(bpath) {
            assert!((p[0] - q[0]).abs() < 0.026 && (p[1] - q[1]).abs() < 0.026);
        }
    }
}
#[test]
fn sample_svg_exports() {
    let d = svg(include_bytes!("../examples/test-design.svg")).unwrap();
    let s = Settings {
        width: 58.,
        height: 28.,
        units: 40.,
        mirror: false,
        rotate: false,
    };
    assert!(export(&d, &s).unwrap().ends_with("!PG;"));
}
#[test]
fn text_is_never_silently_dropped() {
    assert!(svg(br#"<svg xmlns="http://www.w3.org/2000/svg"><text>Hello</text><path d="M0 0L10 10"/></svg>"#).is_err());
}

#[test]
fn all_exporters_preserve_panel_settings_by_omitting_reset_and_force_speed_commands() {
    use mc630_plt_studio::sheet::{CcdTrial, Layout, artwork};
    let art = artwork(
        include_bytes!("../examples/A4-DEMO/sample-sticker.png"),
        false,
        245,
    )
    .unwrap();
    let layout = Layout::default();
    let vector = export(
        &mc630_plt_studio::Drawing::square(),
        &Settings {
            width: 10.,
            height: 10.,
            units: 40.,
            mirror: false,
            rotate: false,
        },
    )
    .unwrap();
    for output in [
        vector,
        layout.plt(&art).unwrap(),
        CcdTrial::default().pair(&layout, &art).unwrap().1,
    ] {
        for command in output.split(';').filter(|s| !s.is_empty()) {
            // Only scan/profile, absolute positions, pen motion and job end are emitted.
            assert!(
                ["TB", "CT", "PA", "PU", "PD", "!PG"]
                    .iter()
                    .any(|op| command.starts_with(op)),
                "Unexpected command: {command}"
            );
            for forbidden in ["IN", "DF", "FS", "VS", "!FS", "!VS"] {
                assert!(
                    !command.starts_with(forbidden),
                    "Panel setting override: {command}"
                );
            }
        }
    }
}

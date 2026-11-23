use crate::perception::SceneState;
pub fn describe(s: &SceneState) -> String {
    if s.status != "tracking" {
        return format!(
            "Capture status: {}. Valid depth covers {:.0}% of samples.",
            s.status,
            s.valid_fraction * 100.
        );
    }
    if s.objects.is_empty() {
        return "No persistent foreground region detected.".into();
    }
    s.objects
        .iter()
        .map(|o| {
            format!(
                "Region {} is {:.2} m forward, {}{}.",
                o.id,
                o.region.z,
                if o.moving { "moving" } else { "stationary" },
                if o.moving && o.direction != "steady" {
                    format!(" and {}", o.direction)
                } else {
                    String::new()
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

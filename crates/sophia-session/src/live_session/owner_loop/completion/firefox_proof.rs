{
    if config.firefox_full_proof_requested() {
        if config.firefox_m10_proof && !firefox_m8_proof.complete() {
            return Err(format!(
                "Firefox M10 promotion proof incomplete: stages={}/{}",
                firefox_m8_proof.completed(),
                firefox_m8_proof.stage_count(),
            )
            .into());
        }
        if config.firefox_m8_proof
            && (!firefox_m8_proof.complete()
                || selection_owner_changes < 2
                || selection_conversions < 2)
        {
            return Err(format!(
                "Firefox M8 proof incomplete: stages={}/{} selection_owner_changes={} selection_conversions={}",
                firefox_m8_proof.completed(),
                firefox_m8_proof.stage_count(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        if config.firefox_m10_proof {
            crate::session_println!(
                "sophia_firefox_promotion schema=1 status=complete stages={} selection_gates=focused content=redacted",
                firefox_m8_proof.completed(),
            );
        } else {
            crate::session_println!(
                "sophia_firefox_m8 schema=1 status=complete stages={} selection_owner_changes={} selection_conversions={} content=redacted",
                firefox_m8_proof.completed(),
                selection_owner_changes,
                selection_conversions,
            );
        }
    }
    if config.firefox_m10_rendering_proof {
        if !firefox_m10_rendering_page_ready {
            return Err("Firefox M10 rendering proof did not observe its ready document".into());
        }
        crate::session_println!(
            "sophia_firefox_rendering schema=1 status=complete page_ready=true recovery_extents=0 content=redacted"
        );
    }
    if config.firefox_m10_dialog_proof {
        if !firefox_m10_dialog_proof.complete() || physical_pointer_buttons_routed < 4 {
            return Err(format!(
                "Firefox M10 dialog proof incomplete: checkpoints={}/{} pointer_buttons={physical_pointer_buttons_routed}",
                firefox_m10_dialog_proof.completed,
                FirefoxM10DialogProof::CHECKPOINTS.len(),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_dialog schema=1 status=complete checkpoints=3 pointer_buttons={physical_pointer_buttons_routed} recovery_extents=0 content=redacted"
        );
    }
    if config.firefox_m10_primary_proof {
        if !firefox_m10_primary_proof.complete()
            || selection_owner_changes < 2
            || selection_conversions < 2
        {
            return Err(format!(
                "Firefox M10 PRIMARY proof incomplete: checkpoints={}/{} selection_owner_changes={} selection_conversions={}",
                firefox_m10_primary_proof.completed,
                FirefoxM10PrimaryProof::CHECKPOINTS.len(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_primary schema=1 status=complete checkpoints=3 selection_owner_changes={selection_owner_changes} selection_conversions={selection_conversions} content=redacted"
        );
    }
    if config.firefox_m10_proof {
        if !firefox_m10_kitty_proof.complete() {
            return Err(format!(
                "Firefox M10 Kitty proof incomplete: checkpoints={}/{}",
                firefox_m10_kitty_proof.completed(),
                FirefoxM10KittyProof::CHECKPOINTS.len(),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_m10 schema=3 status=complete kitty_checkpoints={} selection_gates=focused content=redacted",
            firefox_m10_kitty_proof.completed(),
        );
    }
    if config.firefox_m10_selection_proof {
        if firefox_m8_proof.completed() < 4
            || !firefox_m10_selection_kitty_proof.complete()
            || selection_owner_changes < 4
            || selection_conversions < 4
        {
            return Err(format!(
                "Firefox M10 selection proof incomplete: stages={}/4 checkpoints={}/3 selection_owner_changes={} selection_conversions={}",
                firefox_m8_proof.completed().min(4),
                firefox_m10_selection_kitty_proof.completed(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_selection schema=1 status=complete stages=4 kitty_checkpoints=3 selection_owner_changes={} selection_conversions={} content=redacted",
            selection_owner_changes,
            selection_conversions,
        );
    }
    if config.firefox_m10_lifecycle_proof {
        if !firefox_m8_page_ready_reported || !firefox_m10_kitty_proof.lifecycle_complete() {
            return Err(format!(
                "Firefox M10 lifecycle proof incomplete: page_ready={} checkpoints={}/6",
                firefox_m8_page_ready_reported,
                firefox_m10_kitty_proof.completed().min(6),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_lifecycle schema=1 status=complete page_ready=true kitty_checkpoints=6 content=redacted"
        );
    }
}

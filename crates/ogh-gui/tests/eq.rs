use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ogh_gui::eq::{EqMode, to_mode};
use ogh_gui::presets;
use ogh_gui::{GuiState, Page};
use ogh_ipc::{Connection, Eq, Gate, Request, Setting};
use ogh_protocol::features::advanced_para_eq::{
    self as card, Equalizer, HEADPHONE_DEFAULT, HEADPHONE_GAMING, HEADPHONE_MEDIA,
    HEADPHONE_MIN_FREQ_HZ, MIC_BROADCAST, MIC_COMPETITION, MIC_DEFAULT, MIC_MIN_FREQ_HZ,
};

mod common;
use common::{Window, gui_with, harness, on_page, set, status};

fn convert(card: &Equalizer) -> Eq {
    Eq {
        bands: card
            .bands
            .iter()
            .map(|band| ogh_ipc::EqBand {
                freq_hz: band.freq_hz,
                q: band.q,
                gain_db: band.gain_db,
            })
            .collect(),
    }
}

fn gaming() -> Eq {
    convert(&HEADPHONE_GAMING)
}

fn graphic(flat: &Equalizer, gains_from: &Eq) -> Eq {
    to_mode(gains_from, EqMode::Graphic, &convert(flat))
}

fn headphone_gui(eq: Option<Eq>) -> GuiState {
    let mut status = status(Connection::Connected);
    status.settings.profile.headphone_eq = eq;
    on_page(gui_with(status), Page::Equalizer)
}

fn mic_gui(eq: Option<Eq>, gate: Option<Gate>) -> GuiState {
    let mut status = status(Connection::Connected);
    status.settings.profile.mic_eq = eq;
    status.settings.profile.noise_gate = gate;
    on_page(gui_with(status), Page::Microphone)
}

fn sent_eq(harness: &Harness<'_, Window>) -> Eq {
    match harness.state().sent.as_slice() {
        [
            Request::Set {
                setting: Setting::HeadphoneEq(eq),
            },
        ] => eq.clone(),
        other => panic!("expected one HeadphoneEq, got {other:?}"),
    }
}

fn gain_step(harness: &Harness<'_, Window>) -> Option<f64> {
    harness
        .get_by_role_and_label(Role::Slider, "1 kHz")
        .accesskit_node()
        .numeric_value()
}

fn type_into(harness: &mut Harness<'_, Window>, label: &str, text: &str) {
    harness
        .get_by_role_and_label(Role::SpinButton, label)
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::SpinButton, label)
        .type_text(text);
    harness.key_press(Key::Enter);
    harness.run();
}

#[test]
fn presets_match_the_protocol_cards() {
    let headphone = presets::headphone();
    let mic = presets::mic();
    let names: Vec<_> = headphone
        .iter()
        .chain(&mic)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        names,
        [
            "Default",
            "Gaming",
            "Media",
            "Default",
            "Competition",
            "Broadcast"
        ]
    );

    let cards = [
        (&headphone[0].1, &HEADPHONE_DEFAULT, HEADPHONE_MIN_FREQ_HZ),
        (&headphone[1].1, &HEADPHONE_GAMING, HEADPHONE_MIN_FREQ_HZ),
        (&headphone[2].1, &HEADPHONE_MEDIA, HEADPHONE_MIN_FREQ_HZ),
        (&mic[0].1, &MIC_DEFAULT, MIC_MIN_FREQ_HZ),
        (&mic[1].1, &MIC_COMPETITION, MIC_MIN_FREQ_HZ),
        (&mic[2].1, &MIC_BROADCAST, MIC_MIN_FREQ_HZ),
    ];
    for (eq, card, min_freq_hz) in cards {
        assert_eq!(eq.bands.len(), card::BAND_COUNT);
        for (band, protocol) in eq.bands.iter().zip(&card.bands) {
            assert_eq!(band.freq_hz, protocol.freq_hz);
            assert_eq!(band.q, protocol.q);
            assert_eq!(band.gain_db, protocol.gain_db);
        }
        assert_eq!(eq.validate(min_freq_hz), Ok(()));
    }
}

#[test]
fn to_mode_graphic_resets_q_and_frequencies() {
    let flat = convert(&HEADPHONE_DEFAULT);
    let mut edited = gaming();
    edited.bands[3].freq_hz = 300;
    edited.bands[4].q = Some(2.0);

    let graphic = to_mode(&edited, EqMode::Graphic, &flat);

    for ((band, shown), flat) in graphic.bands.iter().zip(&edited.bands).zip(&flat.bands) {
        assert_eq!(band.q, None);
        assert_eq!(band.freq_hz, flat.freq_hz);
        assert_eq!(band.gain_db, shown.gain_db);
    }
    assert_eq!(graphic.validate(HEADPHONE_MIN_FREQ_HZ), Ok(()));
}

#[test]
fn to_mode_parametric_uses_the_curated_q() {
    let flat = convert(&HEADPHONE_DEFAULT);
    let mut graphic = graphic(&card::HEADPHONE_GAMING, &gaming());
    graphic.bands[2].freq_hz = 130;

    let parametric = to_mode(&graphic, EqMode::Parametric, &flat);

    for (band, shown) in parametric.bands.iter().zip(&graphic.bands) {
        assert_eq!(band.q, Some(0.6875));
        assert_eq!(band.freq_hz, shown.freq_hz);
        assert_eq!(band.gain_db, shown.gain_db);
    }
    assert_eq!(parametric.validate(HEADPHONE_MIN_FREQ_HZ), Ok(()));
}

#[test]
fn gaming_preset_button_sends_headphone_eq() {
    let mut harness = harness(headphone_gui(Some(convert(&HEADPHONE_DEFAULT))));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Gaming")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::HeadphoneEq(gaming()))]
    );
}

#[test]
fn moving_a_band_sends_the_whole_eq() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    harness.get_by_role_and_label(Role::Slider, "1 kHz").focus();
    harness.run();
    harness.key_press(Key::ArrowUp);
    harness.run();

    let sent = sent_eq(&harness);
    let mut expected = gaming();
    expected.bands[5].gain_db += 0.5;
    assert_eq!(sent, expected);
    assert_eq!(sent.validate(HEADPHONE_MIN_FREQ_HZ), Ok(()));
}

#[test]
fn gain_labels_show_frequencies() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    for label in ["20 Hz", "125 Hz", "1 kHz", "2.5 kHz", "20 kHz"] {
        harness.get_by_role_and_label(Role::Slider, label);
    }
}

#[test]
fn parametric_frequency_stays_between_neighbours() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    type_into(&mut harness, "Band 6 frequency", "30000");

    let sent = sent_eq(&harness);
    assert_eq!(sent.bands[5].freq_hz, 2499);
    assert_eq!(sent.bands[6].freq_hz, 2500);
    assert_eq!(sent.validate(HEADPHONE_MIN_FREQ_HZ), Ok(()));
}

#[test]
fn parametric_first_band_keeps_the_target_minimum() {
    let mut mic = convert(&MIC_DEFAULT);
    mic.bands[0].freq_hz = 90;
    let mut harness = harness(mic_gui(Some(mic), None));
    harness.run();

    type_into(&mut harness, "Band 1 frequency", "10");

    let sent = match harness.state().sent.as_slice() {
        [
            Request::Set {
                setting: Setting::MicEq(eq),
            },
        ] => eq.clone(),
        other => panic!("expected one MicEq, got {other:?}"),
    };
    assert_eq!(sent.bands[0].freq_hz, MIC_MIN_FREQ_HZ);
    assert_eq!(sent.validate(MIC_MIN_FREQ_HZ), Ok(()));
}

#[test]
fn parametric_frequency_edit_sends_the_typed_value() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    type_into(&mut harness, "Band 6 frequency", "1200");

    let sent = sent_eq(&harness);
    let mut expected = gaming();
    expected.bands[5].freq_hz = 1200;
    assert_eq!(sent, expected);
}

#[test]
fn committing_an_unedited_q_sends_nothing() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    type_into(&mut harness, "Band 4 Q", "");

    assert!(harness.state().sent.is_empty());
}

#[test]
fn held_gain_slider_ignores_incoming_status() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    let rect = harness.get_by_role_and_label(Role::Slider, "1 kHz").rect();
    let grip = Pos2::new(rect.center().x, rect.top() + rect.height() * 0.2);
    harness.hover_at(grip);
    harness.drag_at(grip);
    harness.run();
    let held = gain_step(&harness).expect("a slider value");
    assert!(held != 16.0 && held != 12.0, "held at {held}");

    let mut flat = status(Connection::Connected);
    flat.settings.profile.headphone_eq = Some(convert(&HEADPHONE_DEFAULT));
    harness.state_mut().gui.shared.status = Some(flat);
    harness.run();
    assert_eq!(gain_step(&harness), Some(held));

    harness.drop_at(grip);
    harness.run();
    assert_eq!(gain_step(&harness), Some(12.0));
}

#[test]
fn parametric_q_is_clamped() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    type_into(&mut harness, "Band 4 Q", "99");

    let sent = sent_eq(&harness);
    assert_eq!(sent.bands[3].q, Some(card::MAX_Q));
    assert_eq!(sent.bands[2].q, Some(0.6875));
}

#[test]
fn graphic_mode_hides_frequency_and_q() {
    let mut harness = harness(headphone_gui(Some(graphic(&HEADPHONE_DEFAULT, &gaming()))));
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::SpinButton, "Band 6 frequency")
            .is_none()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::SpinButton, "Band 6 Q")
            .is_none()
    );
}

#[test]
fn mode_switch_sends_the_converted_eq() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Graphic")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::HeadphoneEq(graphic(
            &HEADPHONE_DEFAULT,
            &gaming()
        )))]
    );
}

#[test]
fn clicking_the_current_mode_sends_nothing() {
    let mut harness = harness(headphone_gui(Some(gaming())));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Parametric")
        .click();
    harness.run();

    assert!(harness.state().sent.is_empty());
}

#[test]
fn unknown_eq_shows_not_read_yet() {
    let mut harness = harness(headphone_gui(None));
    harness.run();

    harness.get_by_label("EQ not read yet");
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "1 kHz")
            .is_none()
    );

    harness.get_by_role_and_label(Role::Button, "Media").click();
    harness.run();
    assert_eq!(
        harness.state().sent,
        vec![set(Setting::HeadphoneEq(convert(&HEADPHONE_MEDIA)))]
    );
}

#[test]
fn eq_controls_are_disabled_when_not_connected() {
    let mut gui = gui_with(status(Connection::GhubRunning));
    gui.page = Page::Equalizer;
    gui.shared
        .status
        .as_mut()
        .unwrap()
        .settings
        .profile
        .headphone_eq = Some(gaming());
    let mut harness = harness(gui);
    harness.run();

    let preset = harness.get_by_role_and_label(Role::Button, "Gaming");
    assert!(preset.accesskit_node().is_disabled());
    preset.click();
    let slider = harness.get_by_role_and_label(Role::Slider, "1 kHz");
    assert!(slider.accesskit_node().is_disabled());
    slider.click();
    harness.run();

    assert!(harness.state().sent.is_empty());
}

#[test]
fn noise_gate_radio_sends_the_gate() {
    let mut harness = harness(mic_gui(Some(convert(&MIC_DEFAULT)), Some(Gate::Off)));
    harness.run();

    harness
        .get_by_role_and_label(Role::RadioButton, "Night")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::NoiseGate(Gate::Night))]
    );
}

#[test]
fn choosing_the_current_gate_sends_nothing() {
    let mut harness = harness(mic_gui(Some(convert(&MIC_DEFAULT)), Some(Gate::Home)));
    harness.run();

    harness
        .get_by_role_and_label(Role::RadioButton, "Home")
        .click();
    harness.run();

    assert!(harness.state().sent.is_empty());
}

#[test]
fn unknown_gate_is_disabled_and_marked() {
    let mut harness = harness(mic_gui(Some(convert(&MIC_DEFAULT)), None));
    harness.run();

    let radio = harness.get_by_role_and_label(Role::RadioButton, "Night");
    assert!(radio.accesskit_node().is_disabled());
    radio.click();
    harness.run();
    harness.get_by_label("not read yet");

    assert!(harness.state().sent.is_empty());
}

#[test]
fn mic_eq_uses_the_mic_setting() {
    let mut harness = harness(mic_gui(Some(convert(&MIC_DEFAULT)), Some(Gate::Off)));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Broadcast")
        .click();
    harness.run();

    assert_eq!(
        harness.state().sent,
        vec![set(Setting::MicEq(convert(&MIC_BROADCAST)))]
    );
    harness.get_by_role_and_label(Role::Slider, "80 Hz");
}

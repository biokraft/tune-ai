use open303_ref::RefOpen303;

#[test]
fn note_on_sounds_and_note_off_decays_to_silence() {
    let mut synth = RefOpen303::new(48000.0);
    synth.note_on(48, 100);
    let head = synth.render(4800);
    assert!(head.iter().all(|x| x.is_finite()));
    assert!(
        head.iter().any(|x| x.abs() > 1e-3),
        "note produced no sound"
    );

    synth.note_on(48, 0);
    let tail = synth.render(96_000);
    assert!(tail.iter().all(|x| x.is_finite()));
    assert!(
        tail[tail.len() - 4800..].iter().all(|x| x.abs() < 1e-4),
        "note did not decay to silence"
    );
}

#[test]
fn trigger_slide_and_release_drive_the_voice() {
    use open303_ref::RefParam;
    let mut synth = RefOpen303::new(48000.0);
    // `BlendOscillator::blend` is uninitialised in C++; set it explicitly.
    synth.set(RefParam::Waveform, 0.0);
    synth.trigger_note(36, true);
    let head = synth.render(4800);
    assert!(head.iter().any(|x| x.abs() > 1e-3), "trigger made no sound");

    synth.slide_to_note(43, false);
    assert!(synth.render(4800).iter().all(|x| x.is_finite()));

    synth.release_note(43);
    let tail = synth.render(96_000);
    assert!(
        tail[tail.len() - 4800..].iter().all(|x| x.abs() < 1e-4),
        "release did not decay to silence"
    );
}

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

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/custom_state.rs");
    t.compile_fail("tests/ui/custom_state_derive_error.rs");
}

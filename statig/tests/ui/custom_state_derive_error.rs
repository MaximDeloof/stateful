#![allow(unused_variables, dead_code, ambiguous_glob_reexports)]

use statig::prelude::*;

#[derive(Default)]
pub struct Blinky {}

pub enum Event {
    TimerElapsed,
    ButtonPressed,
}

pub enum CustomState {
    LedOn,
    LedOff,
}

impl CustomState {
    pub const fn led_on() -> Self {
        Self::LedOn
    }
    pub const fn led_off() -> Self {
        Self::LedOff
    }
}

#[state_machine(
    // This sets the initial state to `led_on`.
    initial = "CustomState::led_on()",
    // Derive the Debug trait on the `State` enum.
    state(derive(Debug)),
    // Derive the Debug trait on the `Superstate` enum.
    superstate(derive(Debug)),
    // Adding custom_state tells the macro not to generate a State type and to
    // instead use the name field to map to a local type.
    state(custom_state, name = "CustomState", derive(Debug))
)]
impl Blinky {
    #[state]
    fn led_on(event: &Event) -> Response<CustomState> {
        Transition(CustomState::led_off())
    }

    #[state]
    fn led_off(event: &Event) -> Response<CustomState> {
        Transition(CustomState::led_on())
    }
}

fn main() {
    let start = std::time::Instant::now();

    let mut state_machine = Blinky::default().state_machine();

    state_machine.handle(&Event::TimerElapsed);
    state_machine.handle(&Event::ButtonPressed);
    state_machine.handle(&Event::TimerElapsed);
    state_machine.handle(&Event::ButtonPressed);

    let end = std::time::Instant::now();

    println!("Duration: {:?}", end - start);
}

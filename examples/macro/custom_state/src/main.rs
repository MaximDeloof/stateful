#![allow(unused)]

use statig::prelude::*;
use std::fmt::Debug;
use std::io::Write;

#[derive(Debug, Default)]
pub struct Blinky;

// The event that will be handled by the state machine.
#[derive(Debug)]
pub enum Event {
    TimerElapsed,
    ButtonPressed,
}

pub enum CustomState {
    NotBlinking,
    LedOff,
    LedOn,
}

impl CustomState {
    pub const fn led_on() -> Self {
        Self::LedOn
    }
}

/// The `state_machine` procedural macro generates the `State` and `Superstate`
/// enums by parsing the function signatures with a `state`, `superstate` or
/// `action` attribute. It also implements the `statig::State` and
/// `statig::Superstate` traits.
#[state_machine(
    // This sets the initial state to `led_on`.
    initial = "CustomState::led_on()",
    // Derive the Debug trait on the `State` enum.
    state(derive(Debug)),
    // Derive the Debug trait on the `Superstate` enum.
    superstate(derive(Debug)),
    // Adding custom_state tells the macro not to generate a State type and to
    // instead use the name field to map to a local type.
    state(custom_state, name = "CustomState")
)]
impl Blinky {
    /// The `#[state]` attibute marks this as a state handler.  By default the
    /// `event` argument will map to the event handler by the state machine.
    /// Every state must return a `Response<CustomState>`.
    #[state(superstate = "blinking")]
    fn led_on(event: &Event) -> Response<CustomState> {
        match event {
            // When we receive a `TimerElapsed` event we transition to the `led_off` state.
            Event::TimerElapsed => Transition(CustomState::LedOff),
            // Other events are deferred to the superstate, in this case `blinking`.
            _ => Super,
        }
    }

    #[state(superstate = "blinking")]
    fn led_off(event: &Event) -> Response<CustomState> {
        match event {
            Event::TimerElapsed => Transition(CustomState::LedOn),
            _ => Super,
        }
    }

    /// The `#[superstate]` attribute marks this as a superstate handler.
    #[superstate]
    fn blinking(event: &Event) -> Response<CustomState> {
        match event {
            Event::ButtonPressed => Transition(CustomState::NotBlinking),
            _ => Super,
        }
    }

    #[state]
    fn not_blinking(event: &Event) -> Response<CustomState> {
        match event {
            Event::ButtonPressed => Transition(CustomState::LedOn),
            // Altough this state has no superstate, we can still defer the event which
            // will cause the event to be handled by an implicit `top` superstate.
            _ => Super,
        }
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

#![allow(unused)]

use statig::awaitable::{self, *};
use std::{
    future::{poll_fn, Future},
    io::Write,
    pin::Pin,
    task::Poll,
};

#[derive(Default)]
pub struct Blinky {
    field: String,
}

// The event that will be handled by the state machine.
pub enum Event {
    TimerElapsed,
    ButtonPressed,
}

// The enum representing all states of the state machine. These are
// the states you can actually transition to.
#[derive(Debug)]
pub enum State {
    LedOn,
    LedOff,
    NotBlinking,
}

// The enum representing the superstates of the system. You can not transition
// to a superstate, but instead they define shared behavior of underlying states or
// superstates.
pub enum Superstate {
    Blinking,
}

// The `statig` trait needs to be implemented on the type that will
// imlement the state machine.
impl IntoStateMachine for Blinky {
    /// The enum that represents the state.
    type State = State;

    type Superstate<'sub> = Superstate;

    /// The event type that will be submitted to the state machine.
    type Event<'evt> = Event;

    type Context<'ctx> = ();

    /// The initial state of the state machine.
    const INITIAL: State = State::LedOn;

    const ON_TRANSITION_ASYNC: for<'fut> fn(
        &'fut mut Self,
        &'fut Self::State,
        &'fut Self::State,
    )
        -> Pin<Box<dyn Future<Output = ()> + Send + 'fut>> = |blinky, from, to| {
        println!("transitioned from {:?} to {:?}", from, to);
        Box::pin(blinky.transition_and_print_internal_state(from, to))
    };
}

// Implement the `statig::State` trait for the state enum.
impl awaitable::State<Blinky> for State {
    fn call_handler<'fut>(
        &'fut mut self,
        blinky: &'fut mut Blinky,
        event: &'fut Event,
        _: &'fut mut (),
    ) -> Pin<Box<(dyn Future<Output = statig::Response<State>> + Send + 'fut)>> {
        match self {
            State::LedOn => Box::pin(Blinky::timer_elapsed_turn_off(event)),
            State::LedOff => Box::pin(Blinky::timer_elapsed_turn_on(event)),
            State::NotBlinking => Box::pin(Blinky::not_blinking_button_pressed(event)),
        }
    }

    fn superstate<'fut>(&mut self) -> Option<Superstate> {
        match self {
            State::LedOn => Some(Superstate::Blinking),
            State::LedOff => Some(Superstate::Blinking),
            State::NotBlinking => None,
        }
    }
}

// Implement the `statig::Superstate` trait for the superstate enum.
impl awaitable::Superstate<Blinky> for Superstate {
    fn call_handler<'fut>(
        &'fut mut self,
        blinky: &'fut mut Blinky,
        event: &'fut Event,
        _: &'fut mut (),
    ) -> Pin<Box<(dyn Future<Output = Response<State>> + Send + 'fut)>> {
        Box::pin(match self {
            Superstate::Blinking => Blinky::blinking_button_pressed(event),
        })
    }
}

impl Blinky {
    async fn transition_and_print_internal_state(&mut self, from: &State, to: &State) {
        println!(
            "transitioned (current test value is: {}) from {:?} to {:?}",
            self.field, from, to
        );
    }
    async fn timer_elapsed_turn_off(event: &Event) -> Response<State> {
        match event {
            Event::TimerElapsed => Transition(State::LedOff),
            _ => Super,
        }
    }

    async fn timer_elapsed_turn_on(event: &Event) -> Response<State> {
        match event {
            Event::TimerElapsed => Transition(State::LedOn),
            _ => Super,
        }
    }

    async fn blinking_button_pressed(event: &Event) -> Response<State> {
        match event {
            Event::ButtonPressed => Transition(State::NotBlinking),
            _ => Super,
        }
    }

    async fn not_blinking_button_pressed(event: &Event) -> Response<State> {
        match event {
            Event::ButtonPressed => Transition(State::LedOn),
            _ => Super,
        }
    }
}

#[tokio::main]
async fn main() {
    let mut state_machine = Blinky {
        field: "test field value".to_string(),
    }
    .state_machine();

    state_machine.handle(&Event::TimerElapsed).await;
    state_machine.handle(&Event::ButtonPressed).await;
    state_machine.handle(&Event::TimerElapsed).await;
    state_machine.handle(&Event::ButtonPressed).await;
}

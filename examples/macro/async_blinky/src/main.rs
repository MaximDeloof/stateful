#![allow(unused)]

use futures::executor;
use futures::future::poll_fn;
use statig::prelude::*;
use std::fmt::Debug;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::thread::spawn;

#[derive(Debug, Default)]
pub struct Blinky;

// The event that will be handled by the state machine.
#[derive(Debug)]
pub enum Event {
    TimerElapsed,
    ButtonPressed,
}

/// The `state_machine` procedural macro generates the `State` and `Superstate`
/// enums by parsing the function signatures with a `state`, `superstate` or
/// `action` attribute. It also implements the `statig::State` and
/// `statig::Superstate` traits.
#[state_machine(
    // This sets the initial state to `led_on`.
    initial = "State::led_on()",
    // Derive the Debug trait on the `State` enum.
    state(derive(Debug)),
    // Derive the Debug trait on the `Superstate` enum.
    superstate(derive(Debug)),
    // Set the `on_transition` callback.
    on_transition = "Self::on_transition",
    // Set the `on_transition_async` callback.
    on_transition_async = "Self::on_transition_async",
    // Set the `on_dispatch` callback.
    on_dispatch = "Self::on_dispatch"
)]
impl Blinky {
    #[action]
    fn cool() {}
    /// The `#[state]` attibute marks this as a state handler.  By default the
    /// `event` argument will map to the event handler by the state machine.
    /// Every state must return a `Response<State>`.
    #[state(superstate = "blinking", entry_action = "cool")]
    async fn led_on(event: &Event) -> Response<State> {
        match event {
            // When we receive a `TimerElapsed` event we transition to the `led_off` state.
            Event::TimerElapsed => Transition(State::led_off()),
            // Other events are deferred to the superstate, in this case `blinking`.
            _ => Super,
        }
    }

    /// Note you can mix sync and async handlers/actions.
    #[state(superstate = "blinking")]
    fn led_off(event: &Event) -> Response<State> {
        match event {
            Event::TimerElapsed => Transition(State::led_on()),
            _ => Super,
        }
    }

    /// The `#[superstate]` attribute marks this as a superstate handler.
    #[superstate]
    async fn blinking(event: &Event) -> Response<State> {
        match event {
            Event::ButtonPressed => Transition(State::not_blinking()),
            _ => Super,
        }
    }

    #[state]
    async fn not_blinking(event: &Event) -> Response<State> {
        match event {
            Event::ButtonPressed => Transition(State::led_on()),
            // Altough this state has no superstate, we can still defer the event which
            // will cause the event to be handled by an implicit `top` superstate.
            _ => Super,
        }
    }
}

impl Blinky {
    // The `on_transition` callback that will be called after every transition.
    fn on_transition(&mut self, source: &State, target: &State) {
        println!("transitioned from `{source:?}` to `{target:?}`");
    }

    async fn transitioning(&mut self, from: &State, to: &State) {
        println!("transitioning from {:?} to {:?}", from, to);
    }

    fn on_transition_async<'a>(
        &'a mut self,
        source: &'a State,
        target: &'a State,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        println!("transitioned async from `{source:?}` to `{target:?}`");
        Box::pin(self.transitioning(source, target))
    }

    fn on_dispatch(&mut self, state: StateOrSuperstate<Self>, event: &Event) {
        println!("dispatching `{event:?}` to `{state:?}`");
    }
}

#[tokio::main]
async fn main() {
    let future = async {
        let mut state_machine = Blinky::default().uninitialized_state_machine().init().await;

        state_machine.handle(&Event::TimerElapsed).await;
        state_machine.handle(&Event::ButtonPressed).await;
        state_machine.handle(&Event::TimerElapsed).await;
        state_machine.handle(&Event::ButtonPressed).await;
    };

    let handle = tokio::spawn(future);

    handle.await;
}

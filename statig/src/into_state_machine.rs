use core::{future::Future, pin::Pin};

use crate::StateOrSuperstate;

/// Trait for transorming a type into a state machine.
pub trait IntoStateMachine
where
    Self: Sized,
{
    /// Event that is processed by the state machine.
    type Event<'evt>;

    /// External context that can be passed in.
    type Context<'ctx>;

    /// Enumeration of the various states.
    type State;

    /// Enumeration of the various superstates.
    type Superstate<'sub>
    where
        Self::State: 'sub;

    /// Initial state of the state machine.
    const INITIAL: Self::State;

    /// Method that is called *before* an event is dispatched to a state or
    /// superstate handler.
    const ON_DISPATCH: fn(&mut Self, StateOrSuperstate<'_, '_, Self>, &Self::Event<'_>) =
        |_, _, _| {};

    /// Method that is called *after* every transition.
    const ON_TRANSITION: fn(&mut Self, &Self::State, &Self::State) = |_, _, _| {};

    const ON_TRANSITION_ASYNC: for<'fut> fn(
        &'fut mut Self,
        from: &'fut Self::State,
        to: &'fut Self::State,
    )
        -> Pin<Box<dyn Future<Output = ()> + Send + 'fut>> = |_, _, _| {
        use std::task::Poll;
        Box::pin(std::future::poll_fn(|_| Poll::Ready(())))
    };
}

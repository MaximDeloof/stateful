use crate::awaitable::IntoStateMachineExt;
#[cfg(feature = "async")]
use crate::awaitable::{self, StateExt as _};
use crate::blocking::{self, StateExt as _};
use crate::{IntoStateMachine, Response};

/// Private internal representation of a state machine that is used for the public types.
pub(crate) struct Inner<M>
where
    M: IntoStateMachine,
{
    pub shared_storage: M,
    pub state: M::State,
}

impl<M> Inner<M>
where
    M: IntoStateMachine,
    M::State: blocking::State<M>,
    for<'sub> M::Superstate<'sub>: blocking::Superstate<M>,
{
    /// Initialize the state machine by executing all entry actions towards the initial state.
    pub fn init_with_context(&mut self, context: &mut M::Context<'_>) {
        let enter_levels = self.state.depth();
        self.state
            .enter(&mut self.shared_storage, context, enter_levels);
    }

    /// Handle the given event.
    pub fn handle_with_context(&mut self, event: &M::Event<'_>, context: &mut M::Context<'_>) {
        let response = self.state.handle(&mut self.shared_storage, event, context);
        match response {
            Response::Super => {}
            Response::Handled => {}
            Response::Transition(state) => self.transition(state, context),
        }
    }

    /// Transition from the current state to the given target state.
    pub fn transition(&mut self, mut target: M::State, context: &mut M::Context<'_>) {
        // Get the transition path we need to perform from one state to the next.
        let (exit_levels, enter_levels) = self.state.transition_path(&mut target);

        // Perform the exit from the previous state towards the common ancestor state.
        self.state
            .exit(&mut self.shared_storage, context, exit_levels);

        // Update the state.
        core::mem::swap(&mut self.state, &mut target);

        // Perform the entry actions from the common ancestor state into the new state.
        self.state
            .enter(&mut self.shared_storage, context, enter_levels);

        M::ON_TRANSITION(&mut self.shared_storage, &target, &self.state);
    }
}

#[cfg(feature = "async")]
impl<M> Inner<M>
where
    M: IntoStateMachineExt,
    for<'evt> M::Event<'evt>: Send + Sync,
    for<'ctx> M::Context<'ctx>: Send + Sync,
    M::State: awaitable::State<M> + Send + 'static,
    for<'sub> M::Superstate<'sub>: awaitable::Superstate<M> + Send,
{
    pub async fn async_init_with_context(&mut self, context: &mut M::Context<'_>) {
        let enter_levels = self.state.depth();
        self.state
            .enter(&mut self.shared_storage, context, enter_levels)
            .await;
    }

    pub async fn async_handle_with_context(
        &mut self,
        event: &M::Event<'_>,
        context: &mut M::Context<'_>,
    ) {
        let response = self
            .state
            .handle(&mut self.shared_storage, event, context)
            .await;
        match response {
            Response::Super => {}
            Response::Handled => {}
            Response::Transition(state) => self.async_transition(state, context).await,
        }
    }

    /// Transition from the current state to the given target state.
    pub async fn async_transition(&mut self, mut target: M::State, context: &mut M::Context<'_>) {
        // Get the transition path we need to perform from one state to the next.
        let (exit_levels, enter_levels) = self.state.transition_path(&mut target);

        // Perform the exit from the previous state towards the common ancestor state.
        self.state
            .exit(&mut self.shared_storage, context, exit_levels)
            .await;

        // Update the state.
        core::mem::swap(&mut self.state, &mut target);

        // Perform the entry actions from the common ancestor state into the new state.
        self.state
            .enter(&mut self.shared_storage, context, enter_levels)
            .await;

        M::ON_TRANSITION(&mut self.shared_storage, &target, &self.state);
        M::ON_TRANSITION_ASYNC(&mut self.shared_storage, &target, &self.state).await;
    }
}

impl<M> Clone for Inner<M>
where
    M: IntoStateMachine + Clone,
    M::State: Clone,
{
    fn clone(&self) -> Self {
        Self {
            shared_storage: self.shared_storage.clone(),
            state: self.state.clone(),
        }
    }
}

impl<M> PartialEq for Inner<M>
where
    M: IntoStateMachine + PartialEq,
    M::State: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.shared_storage == other.shared_storage && self.state == other.state
    }
}

impl<M> Eq for Inner<M>
where
    M: IntoStateMachine + Eq,
    M::State: Eq,
{
}

#[cfg(feature = "serde")]
impl<M> serde::Serialize for Inner<M>
where
    M: IntoStateMachine + serde::Serialize,
    M::State: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut serializer = serializer.serialize_struct("StateMachine", 2)?;
        serializer.serialize_field("shared_storage", &self.shared_storage)?;
        serializer.serialize_field("state", &self.state)?;
        serializer.end()
    }
}

#[cfg(feature = "serde")]
/// A serialized state machine can only be deserialized into an [`UnInitializedStateMachine`] and can
/// then be initialized with [`init`](UnInitializedStateMachine::init).
impl<'de, M> serde::Deserialize<'de> for Inner<M>
where
    M: IntoStateMachine + serde::Deserialize<'de>,
    M::State: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use core::marker::PhantomData;

        enum Field {
            SharedStorage,
            State,
        }

        impl<'de> serde::Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> serde::de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                        formatter.write_str("`shared_storage` or `state`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "shared_storage" => Ok(Field::SharedStorage),
                            "state" => Ok(Field::State),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct InnerVisitor<M>(PhantomData<M>);

        impl<'de, M> serde::de::Visitor<'de> for InnerVisitor<M>
        where
            M: IntoStateMachine + serde::Deserialize<'de>,
            M::State: serde::Deserialize<'de>,
        {
            type Value = Inner<M>;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("StateMachine")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let shared_storage = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let state = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let inner = Inner {
                    shared_storage,
                    state,
                };
                Ok(inner)
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut shared_storage = None;
                let mut state = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SharedStorage => {
                            if shared_storage.is_some() {
                                return Err(serde::de::Error::duplicate_field("shared_storage"));
                            }
                            shared_storage = Some(map.next_value()?);
                        }
                        Field::State => {
                            if state.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state = Some(map.next_value()?);
                        }
                    }
                }
                let shared_storage = shared_storage
                    .ok_or_else(|| serde::de::Error::missing_field("shared_storage"))?;
                let state = state.ok_or_else(|| serde::de::Error::missing_field("state"))?;
                let inner = Inner {
                    shared_storage,
                    state,
                };
                Ok(inner)
            }
        }

        const FIELDS: &[&str] = &["shared_storage", "state"];
        deserializer.deserialize_struct(
            "StateMachine",
            FIELDS,
            InnerVisitor(PhantomData::default()),
        )
    }
}

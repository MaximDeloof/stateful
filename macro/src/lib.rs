extern crate proc_macro;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use proc_macro_error::proc_macro_error;

mod analyze;
mod codegen;
mod lower;
mod parse;
mod visitors;

use analyze::analyze;
use codegen::codegen;
use lower::lower;
use parse::{parse_args, parse_input};

const SUPERSTATE_LIFETIME: &str = "'sub";
const EVENT_LIFETIME: &str = "'event";
const CONTEXT_LIFETIME: &str = "'context";

#[proc_macro_error]
#[proc_macro_attribute]
pub fn state_machine(args: TokenStream, input: TokenStream) -> TokenStream {
    // CH: parsing args, need to add State here
    let attribute_args = parse_args(args.into());
    let item_impl = parse_input(input.into());
    let model = analyze(attribute_args, item_impl);
    let ir = lower(&model);
    let rust = codegen(ir);
    rust.into()
}

#[proc_macro_attribute]
pub fn state(_: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn superstate(_: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn action(_: TokenStream, input: TokenStream) -> TokenStream {
    input
}

use super::*;

fn extract_src(src: &str) -> Extraction {
    extract(src)
}

fn find_def<'a>(e: &'a Extraction, id: &str) -> Option<&'a DefRecord> {
    e.defs.iter().find(|d| d.id == id)
}

mod arg_count_fact;
mod bus_facts;
mod chain_tail_and_lambda_element;
mod def_member_facts;
mod delegate_arg_arity_fact;
mod extension_method_facts;
mod foreach_element_fact;
mod fqn;
mod generic_arity;
mod lambda_slot_fact;
mod method_params_fact;
mod misc_ref_edge_cases;
mod names_list;
mod outer_types_stack;
mod preproc_duplicate_header;
mod preproc_fluent_chain;
mod property_types_fact;
mod qualifier_capture;
mod receiver_facts;
mod registration_fact;
mod stage4_receivers;
mod test_methods_fact;
mod ts_purpose;
mod ts_ref_fragment;
mod using_aliases;

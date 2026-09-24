use super::*;

// The publish fact for one verb. Extraction records a candidate for every
// call that NAMES a message, whatever the method is called, because a
// repository's own forwarding wrapper carries a name this engine cannot know
// in advance -- so a source carrying an unrelated generic call records a
// candidate for that too, and the resolver is what decides which names its
// vocabulary ended up holding.
fn by_verb<'a>(e: &'a Extraction, verb: &str) -> &'a PublishRecord {
    let mut found = e.publishes.iter().filter(|p| p.verb == verb);
    let first = found
        .next()
        .unwrap_or_else(|| panic!("no publish candidate for {verb}: {:?}", e.publishes));
    assert!(
        found.next().is_none(),
        "{verb} must record exactly one candidate: {:?}",
        e.publishes
    );
    first
}

// --- publish-site facts: message resolution per call shape -----------------

#[test]
fn publish_call_records_the_message_type_it_names() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Clerk\n{\n  void Announce(IBus bus)\n  {\n    bus.PublishAsync<LoanRequested>(default);\n  }\n}\n",
    );
    assert_eq!(e.publishes.len(), 1);
    let p = &e.publishes[0];
    assert_eq!(p.verb, "PublishAsync");
    assert_eq!(p.message, "LoanRequested");
    assert_eq!(p.namespace, "App.Bus");
}

#[test]
fn inline_construction_publish_records_the_constructed_message_type() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Desk\n{\n  void Hold(IBus bus)\n  {\n    bus.Publish(new VolumeShelvedMessage { Aisle = \"12B\" }, default);\n  }\n}\n",
    );
    assert_eq!(e.publishes.len(), 1);
    assert_eq!(e.publishes[0].verb, "Publish");
    assert_eq!(e.publishes[0].message, "VolumeShelvedMessage");
}

#[test]
fn identifier_publish_records_the_message_type_of_a_locally_declared_variable() {
    let e = extract_src(
        r#"
namespace App.Bus;

class Clerk
{
  void Confirm(IBus bus)
  {
    var msg = new ReservationHeldMessage();
    var queuedAt = 1;
    LogQueued(queuedAt);
    bus.Publish(msg, default);
  }

  void LogQueued(int at) { }
}
"#,
    );
    assert_eq!(e.publishes.len(), 1);
    assert_eq!(
        e.publishes[0].message, "ReservationHeldMessage",
        "the local's own declared type is read through the ordinary scope ladder, not off the adjacent LogQueued call"
    );
}

#[test]
fn split_line_publish_call_records_one_site_not_several() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Publisher\n{\n  object Announce(IBus bus)\n  {\n    return bus\n      .PublishAsync<CatalogueEntryPublishedMessage>(\n        default);\n  }\n}\n",
    );
    assert_eq!(
        e.publishes.len(),
        1,
        "a call whose own tokens span several source lines is still one site"
    );
    assert_eq!(e.publishes[0].message, "CatalogueEntryPublishedMessage");
}

#[test]
fn saga_initialiser_publish_records_the_message_constructed_inside_its_lambda() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Launcher\n{\n  void Begin(IBus bus)\n  {\n    bus.PublishAsync(context => context.Init(new MembershipLapsedMessage()));\n  }\n}\n",
    );
    let publish = by_verb(&e, "PublishAsync");
    assert_eq!(publish.message, "MembershipLapsedMessage");
}

#[test]
fn mediator_send_records_the_message_type_of_its_own_parameter() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Router\n{\n  void Route(IMediator mediator, CatalogueLookupRequest request)\n  {\n    mediator.Send(request);\n  }\n}\n",
    );
    let publish = by_verb(&e, "Send");
    assert_eq!(publish.message, "CatalogueLookupRequest");
}

// --- refusals ----------------------------------------------------------

#[test]
fn decoy_same_line_bare_name_never_becomes_the_message() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Announcer\n{\n  void Announce(IBus bus) { Telemetry.Tag<LoanRequested>(); bus.Publish(new ExternalNotice(), default); }\n}\n",
    );
    assert_eq!(
        by_verb(&e, "Publish").message,
        "ExternalNotice",
        "the type actually passed to Publish wins over a bare name that merely sits earlier on the same source line"
    );
}

#[test]
fn channel_broadcast_argument_records_no_message_fact() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Broadcaster\n{\n  void Broadcast(IBus bus)\n  {\n    bus.PublishAsync(Channels.ShelfUpdates, default);\n  }\n}\n",
    );
    assert!(
        e.publishes.is_empty(),
        "a dotted channel-name constant is not a type reference, so no message fact is possible"
    );
}

#[test]
fn unrelated_method_named_publish_with_no_invocation_records_nothing() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass ArticleDraft\n{\n  public string Title { get; set; }\n\n  public void Publish()\n  {\n    Title = Title.Trim();\n  }\n}\n",
    );
    assert!(
        e.publishes.is_empty(),
        "a method merely named Publish, never itself invoked on anything, is not a call site"
    );
}

// --- the nested base type-argument fact -------------------------------------

#[test]
fn nested_consumer_base_records_its_inner_type_argument() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass LoanBatchConsumer : IConsumer<Batch<LoanRequested>>\n{\n  void Consume(Batch<LoanRequested> message) { }\n}\n",
    );
    let d = find_def(&e, "App.Bus.LoanBatchConsumer").expect("def present");
    assert_eq!(
        d.base_type_args,
        vec![(
            "IConsumer".to_string(),
            vec!["Batch<LoanRequested>".to_string()]
        )],
        "the inner type argument comes straight off the syntax tree via type_descriptor, not a string match"
    );
    assert_eq!(
        d.base_generic_args,
        vec![("IConsumer".to_string(), vec!["Batch".to_string()])],
        "the flattened sibling fact still reduces the same base to its bare wrapper name alone"
    );
}

#[test]
fn non_nested_generic_base_records_no_base_type_args_entry() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass CatalogueLookupHandler : IRequestHandler<CatalogueLookupRequest, CatalogueLookupResult>\n{\n  void Handle(CatalogueLookupRequest request) { }\n}\n",
    );
    let d = find_def(&e, "App.Bus.CatalogueLookupHandler").expect("def present");
    assert!(
        d.base_type_args.is_empty(),
        "every argument here is a plain identifier, so the flattened baseGenericArgs already carries the whole fact and a second copy would add nothing"
    );
}

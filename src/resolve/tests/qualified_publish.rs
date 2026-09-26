use super::*;

// A publish that names its message through a written qualifier resolves the
// message through that qualifier, so a same-named type in the call site's own
// namespace cannot take the hop away from the consumer the qualifier names.

const COMMAND_IN_ENCLOSING_NAMESPACE: (&str, &str) = (
    "ContainerTests/StartCommand.cs",
    "namespace App.ContainerTests { public class StartCommand { } }",
);

fn redelivery_spec(message: &str) -> String {
    format!(
        "namespace App.ContainerTests\n{{\n    using Subjects;\n\n    public class RedeliverySpec\n    {{\n        public void Run(IBus bus)\n        {{\n            bus.Publish(new {message}());\n        }}\n    }}\n\n    namespace Subjects\n    {{\n        public record StartCommand;\n\n        public class StartCommandConsumer : IConsumer<StartCommand>\n        {{\n        }}\n    }}\n}}\n"
    )
}

fn hops_from<'a>(g: &'a Graph, file: &str) -> Vec<(usize, &'a str, &'a str)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::BusHop {
                from_file,
                from_line,
                message,
                to,
                ..
            } if from_file == file => Some((*from_line, message.as_str(), to.as_str())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_qualified_publish_message_reaches_the_consumer_its_qualifier_names() {
    let spec = redelivery_spec("Subjects.StartCommand");
    let files = fragments_for(&[
        COMMAND_IN_ENCLOSING_NAMESPACE,
        ("ContainerTests/RedeliverySpec.cs", &spec),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        hops_from(&g, "ContainerTests/RedeliverySpec.cs"),
        vec![(
            9,
            "App.ContainerTests.Subjects.StartCommand",
            "App.ContainerTests.Subjects.StartCommandConsumer"
        )]
    );
}

#[test]
fn the_same_publish_written_bare_binds_the_enclosing_namespace_type() {
    let spec = redelivery_spec("StartCommand");
    let files = fragments_for(&[
        COMMAND_IN_ENCLOSING_NAMESPACE,
        ("ContainerTests/RedeliverySpec.cs", &spec),
    ]);
    let g = resolve_graph(&no_git_root(), &files);
    assert!(
        hops_from(&g, "ContainerTests/RedeliverySpec.cs").is_empty(),
        "{:?}",
        hops_from(&g, "ContainerTests/RedeliverySpec.cs")
    );
    assert!(
        type_edge_targets_from(&g, "ContainerTests/RedeliverySpec.cs")
            .contains(&"App.ContainerTests.StartCommand")
    );
}

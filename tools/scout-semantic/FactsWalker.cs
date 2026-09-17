using System.Text.RegularExpressions;

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

namespace ScoutSemantic;

/// <summary>
/// Turns one document into the flow tracer's facts. Framework shapes are matched
/// by simple name and arity only, never by namespace, so a repository that
/// declares its own stand-in types is recognised the same way a referenced
/// package would be.
/// </summary>
internal sealed class FactsWalker
{
    private const int PrefixDepthCap = 8;

    private static readonly Regex MessagesDirectory =
        new(@"(^|/)Messaging/Messages(/|$)", RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex HandlerInterface =
        new(@"^I(Request|Command|Query)Handler$", RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex ControllerToken =
        new(@"\[controller\]", RegexOptions.Compiled | RegexOptions.CultureInvariant | RegexOptions.IgnoreCase);

    private static readonly Regex ActionToken =
        new(@"\[action\]", RegexOptions.Compiled | RegexOptions.CultureInvariant | RegexOptions.IgnoreCase);

    private static readonly HashSet<string> MessageInterfaces = new(StringComparer.Ordinal)
    {
        "ICorrelatedMessage", "IMessage", "CorrelatedBy",
    };

    private static readonly HashSet<string> RegistrationCalls = new(StringComparer.Ordinal)
    {
        "AddScoped", "AddTransient", "AddSingleton", "Register",
    };

    private static readonly Dictionary<string, string> VerbAttributes = new(StringComparer.Ordinal)
    {
        ["HttpGetAttribute"] = "GET",
        ["HttpPostAttribute"] = "POST",
        ["HttpPutAttribute"] = "PUT",
        ["HttpPatchAttribute"] = "PATCH",
        ["HttpDeleteAttribute"] = "DELETE",
    };

    private static readonly Dictionary<string, string> MapVerbs = new(StringComparer.Ordinal)
    {
        ["MapGet"] = "GET",
        ["MapPost"] = "POST",
        ["MapPut"] = "PUT",
        ["MapPatch"] = "PATCH",
        ["MapDelete"] = "DELETE",
    };

    private static readonly HashSet<string> HttpVerbs = new(StringComparer.Ordinal)
    {
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS",
    };

    // Namespace.Outer.Inner<Arg>: fully qualified, no global:: prefix, type
    // arguments spelled out, no nullable annotation.
    private static readonly SymbolDisplayFormat FullyQualified = new(
        globalNamespaceStyle: SymbolDisplayGlobalNamespaceStyle.Omitted,
        typeQualificationStyle: SymbolDisplayTypeQualificationStyle.NameAndContainingTypesAndNamespaces,
        genericsOptions: SymbolDisplayGenericsOptions.IncludeTypeParameters);

    private readonly HashSet<string> _publishCalls;
    private readonly HashSet<string> _consumerBases;

    /// <summary>
    /// Adds the extra publish method names -- each also matched with an
    /// <c>Async</c> suffix -- and the extra consumer base type names to the
    /// built-in defaults.
    /// </summary>
    public FactsWalker(IEnumerable<string> extraPublishCalls, IEnumerable<string> extraConsumerBases)
    {
        _publishCalls = new HashSet<string>(StringComparer.Ordinal) { "Publish", "PublishAsync" };
        foreach (var name in extraPublishCalls)
        {
            _publishCalls.Add(name);
            _publishCalls.Add(name + "Async");
        }

        _consumerBases = new HashSet<string>(StringComparer.Ordinal) { "IConsumer", "BaseConsumer" };
        foreach (var name in extraConsumerBases)
        {
            _consumerBases.Add(name);
        }
    }

    /// <summary>Recognised sites whose type could not be resolved into a fact.</summary>
    public int Unresolved { get; private set; }

    /// <summary>
    /// Counts one more unresolved site. The caller uses it for a document whose
    /// walk threw, so a failed document is never silently dropped from the
    /// strict check.
    /// </summary>
    public void CountUnresolved() => Unresolved++;

    /// <summary>Appends every fact found in one document to <paramref name="sink"/>.</summary>
    public void WalkDocument(SemanticModel model, SyntaxTree tree, string relFile, List<FactRecord> sink)
    {
        foreach (var node in tree.GetRoot().DescendantNodes())
        {
            switch (node)
            {
                case TypeDeclarationSyntax type:
                    WalkType(model, type, relFile, sink);
                    break;
                case MethodDeclarationSyntax method:
                    WalkMethod(model, method, relFile, sink);
                    break;
                case ConstructorDeclarationSyntax constructor:
                    WalkConstructor(model, constructor, relFile, sink);
                    break;
                case InvocationExpressionSyntax invocation:
                    WalkInvocation(model, invocation, relFile, sink);
                    break;
            }
        }
    }

    // ----------------------------------------------------------------- types

    private void WalkType(SemanticModel model, TypeDeclarationSyntax declaration, string relFile, List<FactRecord> sink)
    {
        if (model.GetDeclaredSymbol(declaration) is not { TypeKind: TypeKind.Class } type)
        {
            return;
        }

        var identifierLine = LineOf(declaration.Identifier);
        var bodyLine = declaration.OpenBraceToken.IsKind(SyntaxKind.OpenBraceToken)
            ? LineOf(declaration.OpenBraceToken)
            : identifierLine;

        // A partial type is one symbol spread over several declarations. The
        // kinds below describe the symbol, not the part they are read from, so
        // they are emitted once, at the canonical part. The syntactic kinds --
        // iface_impl and ctor_field -- describe the part itself and stay where
        // they are written.
        var canonical = IsCanonicalPart(type, declaration);

        if (canonical
            && (MessagesDirectory.IsMatch(relFile)
                || type.Name.EndsWith("Message", StringComparison.Ordinal)
                || type.AllInterfaces.Any(i => MessageInterfaces.Contains(i.Name))))
        {
            sink.Add(new FactRecord("message_class", relFile, bodyLine)
                .With("name", type.Name)
                .With("fqn", Fqn(type)));
        }

        // The written base list only. A positional record's IEquatable<T> is
        // synthesised by the compiler, never spelled out in the source, and so
        // is not a fact about what this declaration implements.
        if (declaration.BaseList is { } baseList)
        {
            var declared = new HashSet<string>(StringComparer.Ordinal);
            foreach (var baseType in baseList.Types)
            {
                if (model.GetTypeInfo(baseType.Type).Type is not { TypeKind: TypeKind.Interface } iface)
                {
                    continue;
                }

                var ifaceFqn = Fqn(iface);
                if (!declared.Add(ifaceFqn))
                {
                    continue;
                }

                sink.Add(new FactRecord("iface_impl", relFile, identifierLine)
                    .With("class", type.Name)
                    .With("iface", iface.Name)
                    .With("ifaceFqn", ifaceFqn));
            }
        }

        if (canonical && !type.IsAbstract)
        {
            EmitConsume(type, relFile, identifierLine, sink);
            EmitHandlerBindings(type, relFile, identifierLine, sink);
        }

        if (declaration.ParameterList is { } primary)
        {
            foreach (var parameter in primary.Parameters)
            {
                EmitPrimaryCtorField(model, type.Name, parameter, relFile, bodyLine, sink);
            }
        }
    }

    /// <summary>
    /// True when <paramref name="declaration"/> is the part of a partial type
    /// that carries its symbol-derived facts: the first, ordered by file path
    /// then position, among the parts that declare a base list -- and among all
    /// parts when none does.
    /// </summary>
    private static bool IsCanonicalPart(INamedTypeSymbol type, TypeDeclarationSyntax declaration)
    {
        if (type.DeclaringSyntaxReferences.Length < 2)
        {
            return true;
        }

        TypeDeclarationSyntax? best = null;
        var bestDeclaresBases = false;
        foreach (var reference in type.DeclaringSyntaxReferences)
        {
            if (reference.GetSyntax() is not TypeDeclarationSyntax part)
            {
                continue;
            }

            var declaresBases = part.BaseList is not null;
            if (best is null || (declaresBases && !bestDeclaresBases))
            {
                best = part;
                bestDeclaresBases = declaresBases;
                continue;
            }

            if (declaresBases != bestDeclaresBases)
            {
                continue;
            }

            var byPath = string.CompareOrdinal(part.SyntaxTree.FilePath, best.SyntaxTree.FilePath);
            if (byPath < 0 || (byPath == 0 && part.SpanStart < best.SpanStart))
            {
                best = part;
            }
        }

        return best is null || best == declaration;
    }

    private void EmitConsume(INamedTypeSymbol type, string relFile, int line, List<FactRecord> sink)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var candidate in BasesAndInterfaces(type))
        {
            if (!_consumerBases.Contains(candidate.Name) || candidate.TypeArguments.Length != 1)
            {
                continue;
            }

            var message = candidate.TypeArguments[0];
            if (message is INamedTypeSymbol { Name: "Batch" } batch && batch.TypeArguments.Length == 1)
            {
                message = batch.TypeArguments[0];
            }

            if (message.TypeKind == TypeKind.TypeParameter)
            {
                continue;
            }

            if (message.TypeKind == TypeKind.Error)
            {
                Unresolved++;
                continue;
            }

            if (message.Name.Length == 0 || !seen.Add(message.Name))
            {
                continue;
            }

            sink.Add(new FactRecord("consume", relFile, line)
                .With("message", message.Name)
                .With("consumer", type.Name)
                .With("fqn", Fqn(message)));
        }
    }

    private static void EmitHandlerBindings(INamedTypeSymbol type, string relFile, int line, List<FactRecord> sink)
    {
        foreach (var iface in type.AllInterfaces)
        {
            if (!HandlerInterface.IsMatch(iface.Name) || iface.TypeArguments.Length < 1)
            {
                continue;
            }

            var request = iface.TypeArguments[0];
            sink.Add(new FactRecord("di_binding", relFile, line)
                .With("iface", request.Name)
                .With("impl", type.Name)
                .With("ifaceFqn", Fqn(request))
                .With("implFqn", Fqn(type)));
        }
    }

    private static IEnumerable<INamedTypeSymbol> BasesAndInterfaces(INamedTypeSymbol type)
    {
        for (INamedTypeSymbol? current = type; current is not null; current = current.BaseType)
        {
            yield return current;
        }

        foreach (var iface in type.AllInterfaces)
        {
            yield return iface;
        }
    }

    // --------------------------------------------------------------- members

    private void WalkMethod(SemanticModel model, MethodDeclarationSyntax method, string relFile, List<FactRecord> sink)
    {
        if (method.Parent is not TypeDeclarationSyntax owner
            || model.GetDeclaredSymbol(owner) is not { IsImplicitlyDeclared: false } ownerType)
        {
            return;
        }

        EmitMethodSpan(relFile, ownerType.Name, method.Identifier.ValueText, method.Identifier, method, sink);
        if (ownerType.TypeKind == TypeKind.Class)
        {
            EmitAttributeRoutes(model, method, owner, ownerType.Name, relFile, sink);
            EmitMethodCalls(model, method, ownerType, relFile, sink);
        }
    }

    private void WalkConstructor(
        SemanticModel model, ConstructorDeclarationSyntax constructor, string relFile, List<FactRecord> sink)
    {
        if (constructor.Parent is not TypeDeclarationSyntax owner
            || model.GetDeclaredSymbol(owner) is not { IsImplicitlyDeclared: false } ownerType)
        {
            return;
        }

        EmitMethodSpan(relFile, ownerType.Name, ownerType.Name, constructor.Identifier, constructor, sink);
        EmitConstructorFields(model, constructor, ownerType, relFile, sink);
    }

    private static void EmitMethodSpan(
        string relFile,
        string className,
        string methodName,
        SyntaxToken identifier,
        BaseMethodDeclarationSyntax declaration,
        List<FactRecord> sink)
    {
        int endLine;
        if (declaration.Body is { } block)
        {
            endLine = LineOf(block.CloseBraceToken);
        }
        else if (declaration.ExpressionBody is not null)
        {
            endLine = LineOf(declaration.SemicolonToken);
        }
        else
        {
            return;
        }

        sink.Add(new FactRecord("method_span", relFile, LineOf(identifier))
            .With("class", className)
            .With("method", methodName)
            .With("endLine", endLine));
    }

    private void EmitConstructorFields(
        SemanticModel model,
        ConstructorDeclarationSyntax constructor,
        INamedTypeSymbol owner,
        string relFile,
        List<FactRecord> sink)
    {
        if (constructor.ParameterList.Parameters.Count == 0
            || model.GetDeclaredSymbol(constructor) is not { } symbol)
        {
            return;
        }

        SyntaxNode? body = constructor.Body ?? (SyntaxNode?)constructor.ExpressionBody;
        if (body is null)
        {
            return;
        }

        var parameters = new HashSet<ISymbol>(symbol.Parameters, SymbolEqualityComparer.Default);
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var assignment in Inside(body).OfType<AssignmentExpressionSyntax>())
        {
            if (!assignment.IsKind(SyntaxKind.SimpleAssignmentExpression))
            {
                continue;
            }

            if (ParameterOf(model, assignment.Right, parameters) is not { } parameter
                || MemberOf(model, assignment.Left, owner) is not { } member
                || !seen.Add(member.Name + " " + parameter.Name))
            {
                continue;
            }

            if (parameter.Type.TypeKind == TypeKind.Error)
            {
                Unresolved++;
                continue;
            }

            sink.Add(CtorField(relFile, LineOf(assignment), owner.Name, member.Name, parameter.Type));
        }
    }

    /// <summary>
    /// The already-declared, already-consumer-documented <c>method_call</c>
    /// slot (<c>class</c>, <c>method</c>, <c>field</c>, <c>calledMethod</c>):
    /// a body calling a member on a constructor-injected field, the exact
    /// scope <c>docs/fact-schema.md</c> (flowtrace-cli) publishes for this
    /// kind. Narrower than every field access this file could in principle
    /// resolve: the receiver must be exactly one field <see
    /// cref="InjectedFields"/> proves was ctor-injected, not an arbitrary
    /// member or a chained call.
    /// </summary>
    private void EmitMethodCalls(
        SemanticModel model, MethodDeclarationSyntax method, INamedTypeSymbol owner, string relFile, List<FactRecord> sink)
    {
        if (method.Body is not { } body)
        {
            return;
        }

        var injected = InjectedFields(model, owner);
        if (injected.Count == 0)
        {
            return;
        }

        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var invocation in Inside(body).OfType<InvocationExpressionSyntax>())
        {
            if (ReceiverOf(invocation.Expression) is not { } receiver
                || MemberOf(model, receiver, owner) is not { } field
                || !injected.Contains(field)
                || InvokedName(model, invocation) is not { } calledMethod
                || !seen.Add(field.Name + " " + calledMethod + " " + LineOf(invocation)))
            {
                continue;
            }

            sink.Add(new FactRecord("method_call", relFile, LineOf(invocation))
                .With("class", owner.Name)
                .With("method", method.Identifier.ValueText)
                .With("field", field.Name)
                .With("calledMethod", calledMethod));
        }
    }

    /// <summary>
    /// Every field/property of <paramref name="owner"/> that some constructor
    /// of it assigns straight from one of that constructor's own parameters
    /// (the same <c>_x = x</c> / null-guard shape <see
    /// cref="EmitConstructorFields"/> recognises for <c>ctor_field</c>) --
    /// the set <see cref="EmitMethodCalls"/> treats as "injected". Only a
    /// constructor declared in <paramref name="model"/>'s OWN syntax tree is
    /// read: a Roslyn <see cref="SemanticModel"/> only answers for the tree
    /// it was built from, so a constructor-injected field assigned in a
    /// DIFFERENT file of a partial class is a known, accepted miss here
    /// rather than a second model lookup this sidecar does not otherwise need.
    /// </summary>
    private static HashSet<ISymbol> InjectedFields(SemanticModel model, INamedTypeSymbol owner)
    {
        var fields = new HashSet<ISymbol>(SymbolEqualityComparer.Default);
        foreach (var constructor in owner.Constructors)
        {
            if (constructor.Parameters.Length == 0)
            {
                continue;
            }

            foreach (var reference in constructor.DeclaringSyntaxReferences)
            {
                if (reference.SyntaxTree != model.SyntaxTree
                    || reference.GetSyntax() is not ConstructorDeclarationSyntax declaration)
                {
                    continue;
                }

                SyntaxNode? body = declaration.Body ?? (SyntaxNode?)declaration.ExpressionBody;
                if (body is null)
                {
                    continue;
                }

                var parameters = new HashSet<ISymbol>(constructor.Parameters, SymbolEqualityComparer.Default);
                foreach (var assignment in Inside(body).OfType<AssignmentExpressionSyntax>())
                {
                    if (assignment.IsKind(SyntaxKind.SimpleAssignmentExpression)
                        && ParameterOf(model, assignment.Right, parameters) is not null
                        && MemberOf(model, assignment.Left, owner) is { } member)
                    {
                        fields.Add(member);
                    }
                }
            }
        }

        return fields;
    }

    private void EmitPrimaryCtorField(
        SemanticModel model,
        string className,
        ParameterSyntax parameter,
        string relFile,
        int line,
        List<FactRecord> sink)
    {
        var type = model.GetDeclaredSymbol(parameter)?.Type;
        if (type is null || type.TypeKind == TypeKind.Error)
        {
            Unresolved++;
            return;
        }

        sink.Add(CtorField(relFile, line, className, parameter.Identifier.ValueText, type));
    }

    private static FactRecord CtorField(string relFile, int line, string className, string field, ITypeSymbol type) =>
        new FactRecord("ctor_field", relFile, line)
            .With("class", className)
            .With("field", field)
            .With("paramType", type.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat))
            .With("paramTypeFqn", Fqn(type));

    private static IParameterSymbol? ParameterOf(
        SemanticModel model, ExpressionSyntax right, HashSet<ISymbol> parameters)
    {
        // `_x = x` and the `_x = x ?? throw ...` guard shape.
        var expression = right is BinaryExpressionSyntax binary && binary.IsKind(SyntaxKind.CoalesceExpression)
            ? binary.Left
            : right;

        if (expression is not IdentifierNameSyntax)
        {
            return null;
        }

        return model.GetSymbolInfo(expression).Symbol is IParameterSymbol parameter && parameters.Contains(parameter)
            ? parameter
            : null;
    }

    private static ISymbol? MemberOf(SemanticModel model, ExpressionSyntax left, INamedTypeSymbol owner)
    {
        var expression = left is MemberAccessExpressionSyntax { Expression: ThisExpressionSyntax } access
            ? access.Name
            : left;

        if (expression is not SimpleNameSyntax)
        {
            return null;
        }

        var symbol = model.GetSymbolInfo(expression).Symbol;
        if (symbol is not (IFieldSymbol or IPropertySymbol))
        {
            return null;
        }

        return SymbolEqualityComparer.Default.Equals(symbol.ContainingType, owner) ? symbol : null;
    }

    // ---------------------------------------------------------------- routes

    private static void EmitAttributeRoutes(
        SemanticModel model,
        MethodDeclarationSyntax method,
        TypeDeclarationSyntax owner,
        string controller,
        string relFile,
        List<FactRecord> sink)
    {
        var verbs = new List<(string Verb, AttributeSyntax Attribute)>();
        var routes = new List<AttributeSyntax>();
        foreach (var list in method.AttributeLists)
        {
            foreach (var attribute in list.Attributes)
            {
                var name = AttributeName(model, attribute);
                if (name is null)
                {
                    continue;
                }

                if (VerbAttributes.TryGetValue(name, out var verb))
                {
                    verbs.Add((verb, attribute));
                }
                else if (name == "RouteAttribute")
                {
                    routes.Add(attribute);
                }
            }
        }

        if (verbs.Count == 0 && routes.Count == 0)
        {
            return;
        }

        var classTemplate = ClassTemplate(model, owner);
        var action = method.Identifier.ValueText;
        var bare = new List<(string Verb, AttributeSyntax Attribute)>();

        foreach (var (verb, attribute) in verbs)
        {
            var argument = PositionalArgument(attribute);
            if (argument is null)
            {
                bare.Add((verb, attribute));
                continue;
            }

            var template = Expand(Join(classTemplate, StringConstant(model, argument)), controller, action);
            sink.Add(Route(relFile, LineOf(attribute), controller, action, verb, template));
        }

        foreach (var attribute in routes)
        {
            var argument = PositionalArgument(attribute);
            var literal = argument is null ? "" : StringConstant(model, argument);
            var template = Expand(Join(classTemplate, literal), controller, action);
            var line = LineOf(attribute);
            if (bare.Count == 0)
            {
                sink.Add(Route(relFile, line, controller, action, "ANY", template));
                continue;
            }

            foreach (var (verb, _) in bare)
            {
                sink.Add(Route(relFile, line, controller, action, verb, template));
            }
        }

        if (routes.Count == 0)
        {
            var template = Expand(Join(classTemplate, ""), controller, action);
            foreach (var (verb, attribute) in bare)
            {
                sink.Add(Route(relFile, LineOf(attribute), controller, action, verb, template));
            }
        }
    }

    private void EmitMinimalApi(
        SemanticModel model, InvocationExpressionSyntax invocation, string call, string relFile, List<FactRecord> sink)
    {
        var arguments = invocation.ArgumentList.Arguments;
        if (arguments.Count == 0 || model.GetConstantValue(arguments[0].Expression).Value is not string literal)
        {
            return;
        }

        var visited = new HashSet<ISymbol>(SymbolEqualityComparer.Default);
        var template = Join(Prefix(model, ReceiverOf(invocation.Expression), 0, visited), literal);
        var verbs = new List<string>();
        int handlerIndex;

        if (call == "MapMethods")
        {
            handlerIndex = 2;
            if (arguments.Count > 1)
            {
                foreach (var node in arguments[1].Expression.DescendantNodesAndSelf().OfType<ExpressionSyntax>())
                {
                    if (model.GetConstantValue(node).Value is not string text)
                    {
                        continue;
                    }

                    var verb = text.ToUpperInvariant();
                    if (HttpVerbs.Contains(verb) && !verbs.Contains(verb))
                    {
                        verbs.Add(verb);
                    }
                }
            }

            if (verbs.Count == 0)
            {
                verbs.Add("ANY");
            }
        }
        else
        {
            handlerIndex = 1;
            verbs.Add(MapVerbs[call]);
        }

        var controller = ControllerName(model, invocation, relFile);
        var handler = arguments.Count > handlerIndex ? arguments[handlerIndex].Expression : null;
        var action = ActionName(model, invocation, call, handler);
        var line = LineOf(invocation);

        foreach (var verb in verbs)
        {
            sink.Add(Route(relFile, line, controller, action, verb, template));
        }

        if (handler is not LambdaExpressionSyntax lambda)
        {
            return;
        }

        // A lambda registered from inside a method or constructor already lies
        // within that member's span, and a second span under the same
        // class+method would hide the outer range from a consumer that joins on
        // the pair. Only a lambda with no enclosing member -- top-level
        // statements, a field or property initialiser -- carries its own span.
        if (!lambda.Ancestors().Any(a => a is MethodDeclarationSyntax or ConstructorDeclarationSyntax))
        {
            sink.Add(new FactRecord("method_span", relFile, LineOf(lambda))
                .With("class", controller)
                .With("method", action)
                .With("endLine", EndLineOf(lambda)));
        }

        foreach (var parameter in LambdaParameters(lambda))
        {
            var type = model.GetDeclaredSymbol(parameter)?.Type;
            if (type is null || IsFrameworkShape(type))
            {
                continue;
            }

            sink.Add(CtorField(relFile, LineOf(lambda), controller, parameter.Identifier.ValueText, type));
        }
    }

    private static FactRecord Route(
        string relFile, int line, string controller, string action, string verb, string template) =>
        new FactRecord("route", relFile, line)
            .With("controller", controller)
            .With("action", action)
            .With("verb", verb)
            .With("template", template);

    private static string ClassTemplate(SemanticModel model, TypeDeclarationSyntax owner)
    {
        foreach (var list in owner.AttributeLists)
        {
            foreach (var attribute in list.Attributes)
            {
                if (AttributeName(model, attribute) != "RouteAttribute")
                {
                    continue;
                }

                var argument = PositionalArgument(attribute);
                return argument is null ? "" : StringConstant(model, argument);
            }
        }

        return "";
    }

    private static string? AttributeName(SemanticModel model, AttributeSyntax attribute)
    {
        if (model.GetSymbolInfo(attribute).Symbol?.ContainingType is { } declared)
        {
            return declared.Name;
        }

        var name = attribute.Name switch
        {
            QualifiedNameSyntax qualified => qualified.Right.Identifier.ValueText,
            AliasQualifiedNameSyntax aliased => aliased.Name.Identifier.ValueText,
            SimpleNameSyntax simple => simple.Identifier.ValueText,
            _ => null,
        };

        if (name is null)
        {
            return null;
        }

        return name.EndsWith("Attribute", StringComparison.Ordinal) ? name : name + "Attribute";
    }

    private static ExpressionSyntax? PositionalArgument(AttributeSyntax attribute) =>
        attribute.ArgumentList?.Arguments
            .FirstOrDefault(a => a.NameEquals is null && a.NameColon is null)?.Expression;

    private static string StringConstant(SemanticModel model, ExpressionSyntax expression) =>
        model.GetConstantValue(expression).Value as string ?? "";

    private static string Join(string left, string right)
    {
        var parts = new List<string>(2);
        var head = left.Trim('/');
        if (head.Length > 0)
        {
            parts.Add(head);
        }

        var tail = right.Trim('/');
        if (tail.Length > 0)
        {
            parts.Add(tail);
        }

        return string.Join('/', parts);
    }

    private static string Expand(string template, string controller, string action)
    {
        const string suffix = "Controller";
        var name = controller.Length > suffix.Length && controller.EndsWith(suffix, StringComparison.Ordinal)
            ? controller[..^suffix.Length]
            : controller;

        return ActionToken.Replace(ControllerToken.Replace(template, _ => name), _ => action);
    }

    private static string ControllerName(SemanticModel model, SyntaxNode node, string relFile)
    {
        var owner = node.Ancestors().OfType<TypeDeclarationSyntax>().FirstOrDefault();
        if (owner is not null
            && model.GetDeclaredSymbol(owner) is { IsImplicitlyDeclared: false } type
            && !type.Name.StartsWith('<'))
        {
            return type.Name;
        }

        return Path.GetFileNameWithoutExtension(relFile);
    }

    private static string ActionName(
        SemanticModel model, InvocationExpressionSyntax invocation, string call, ExpressionSyntax? handler)
    {
        if (handler is IdentifierNameSyntax or MemberAccessExpressionSyntax)
        {
            var info = model.GetSymbolInfo(handler);
            var method = info.Symbol as IMethodSymbol ?? info.CandidateSymbols.OfType<IMethodSymbol>().FirstOrDefault();
            if (method is not null)
            {
                return method.Name;
            }
        }

        var enclosing = invocation.Ancestors().OfType<MethodDeclarationSyntax>().FirstOrDefault();
        return enclosing?.Identifier.ValueText ?? call;
    }

    /// <summary>
    /// The route prefix a chain of <c>MapGroup</c> receivers contributes.
    /// <paramref name="visited"/> holds the group symbols already followed, so
    /// two initialisers that name each other yield "" instead of a fabricated
    /// template.
    /// </summary>
    private string Prefix(SemanticModel model, ExpressionSyntax? receiver, int depth, HashSet<ISymbol> visited)
    {
        if (receiver is null || depth >= PrefixDepthCap)
        {
            return "";
        }

        if (receiver is InvocationExpressionSyntax group)
        {
            if (InvokedName(model, group) != "MapGroup"
                || group.ArgumentList.Arguments.Count == 0
                || model.GetConstantValue(group.ArgumentList.Arguments[0].Expression).Value is not string literal)
            {
                return "";
            }

            return Join(Prefix(model, ReceiverOf(group.Expression), depth + 1, visited), literal);
        }

        if (receiver is not (IdentifierNameSyntax or MemberAccessExpressionSyntax))
        {
            return "";
        }

        var symbol = model.GetSymbolInfo(receiver).Symbol;
        if (symbol is not (ILocalSymbol or IFieldSymbol or IPropertySymbol) || !visited.Add(symbol))
        {
            return "";
        }

        var (declaringModel, initializer, foreign) = InitializerOf(model, symbol);
        if (foreign)
        {
            return ForeignPrefix(initializer, depth + 1);
        }

        return initializer is InvocationExpressionSyntax declared
            ? Prefix(declaringModel, declared, depth + 1, visited)
            : "";
    }

    /// <summary>
    /// The prefix of a group declared in a referenced project. That tree belongs
    /// to another compilation, so nothing here may be bound and the receiver
    /// chain is read as syntax: every <c>MapGroup</c> link carrying a single
    /// string literal contributes it, and links of any other name -- whatever
    /// built the root group -- contribute nothing. Only a <c>MapGroup</c> whose
    /// argument is not such a literal is unresolved: an initialiser with no
    /// <c>MapGroup</c> at all has no prefix to lose and is silent.
    /// </summary>
    private string ForeignPrefix(ExpressionSyntax? initializer, int depth)
    {
        // Outermost link first, so the collected literals are reversed below.
        var groups = new List<string>();
        var current = initializer;
        for (var step = depth; step < PrefixDepthCap && current is InvocationExpressionSyntax call; step++)
        {
            if (SyntacticName(call) == "MapGroup")
            {
                if (call.ArgumentList.Arguments.Count != 1
                    || call.ArgumentList.Arguments[0].Expression is not LiteralExpressionSyntax literal
                    || !literal.IsKind(SyntaxKind.StringLiteralExpression))
                {
                    Unresolved++;
                    return "";
                }

                groups.Add(literal.Token.ValueText);
            }

            current = ReceiverOf(call.Expression);
        }

        var prefix = "";
        for (var i = groups.Count - 1; i >= 0; i--)
        {
            prefix = Join(prefix, groups[i]);
        }

        return prefix;
    }

    /// <summary>
    /// The expression a group symbol is declared with, and the model that can
    /// bind it. A property contributes either its initialiser or its
    /// expression-bodied getter: both spell the group the same way, and the
    /// getter form is what a chain of groups that name each other has to use.
    /// <c>Foreign</c> marks an expression whose tree belongs to another
    /// compilation -- a group declared in a referenced project -- which no model
    /// here may bind and which <see cref="ForeignPrefix"/> reads as syntax.
    /// </summary>
    private static (SemanticModel Model, ExpressionSyntax? Initializer, bool Foreign) InitializerOf(
        SemanticModel model, ISymbol symbol)
    {
        foreach (var reference in symbol.DeclaringSyntaxReferences)
        {
            var value = reference.GetSyntax() switch
            {
                VariableDeclaratorSyntax variable => variable.Initializer?.Value,
                PropertyDeclarationSyntax property =>
                    property.Initializer?.Value ?? property.ExpressionBody?.Expression,
                _ => null,
            };

            if (value is null)
            {
                continue;
            }

            var tree = reference.SyntaxTree;
            if (tree == model.SyntaxTree)
            {
                return (model, value, false);
            }

            if (!model.Compilation.ContainsSyntaxTree(tree))
            {
                return (model, value, true);
            }

            return (model.Compilation.GetSemanticModel(tree), value, false);
        }

        return (model, null, false);
    }

    private static IEnumerable<ParameterSyntax> LambdaParameters(LambdaExpressionSyntax lambda) => lambda switch
    {
        SimpleLambdaExpressionSyntax simple => new[] { simple.Parameter },
        ParenthesizedLambdaExpressionSyntax parenthesized => parenthesized.ParameterList.Parameters,
        _ => Enumerable.Empty<ParameterSyntax>(),
    };

    /// <summary>
    /// True for the handler parameters a minimal-API lambda takes from the
    /// framework rather than from the request: primitives, enums, nullable value
    /// types, unresolved types, and anything under a System or Microsoft
    /// namespace.
    /// </summary>
    private static bool IsFrameworkShape(ITypeSymbol type)
    {
        if (type.SpecialType != SpecialType.None
            || type.TypeKind is TypeKind.Enum or TypeKind.TypeParameter or TypeKind.Error)
        {
            return true;
        }

        if (type is not INamedTypeSymbol named)
        {
            return false;
        }

        if (named.OriginalDefinition.SpecialType == SpecialType.System_Nullable_T)
        {
            return true;
        }

        if (named.ContainingNamespace is not { IsGlobalNamespace: false } ns)
        {
            return false;
        }

        var qualified = ns.ToDisplayString();
        return IsFrameworkNamespace(qualified, "System") || IsFrameworkNamespace(qualified, "Microsoft");
    }

    /// <summary>
    /// True when <paramref name="qualified"/> is <paramref name="root"/> or one
    /// of its descendants. A bare prefix test would also swallow an unrelated
    /// namespace that merely starts with those letters.
    /// </summary>
    private static bool IsFrameworkNamespace(string qualified, string root) =>
        string.Equals(qualified, root, StringComparison.Ordinal)
        || qualified.StartsWith(root + ".", StringComparison.Ordinal);

    // ----------------------------------------------------------- invocations

    private void WalkInvocation(
        SemanticModel model, InvocationExpressionSyntax invocation, string relFile, List<FactRecord> sink)
    {
        var name = InvokedName(model, invocation);
        if (name is null)
        {
            return;
        }

        if (_publishCalls.Contains(name))
        {
            EmitPublish(model, invocation, relFile, sink);
        }

        if (RegistrationCalls.Contains(name))
        {
            EmitDiBinding(model, invocation, relFile, sink);
        }

        if (MapVerbs.ContainsKey(name) || name == "MapMethods")
        {
            EmitMinimalApi(model, invocation, name, relFile, sink);
        }
    }

    private void EmitPublish(
        SemanticModel model, InvocationExpressionSyntax invocation, string relFile, List<FactRecord> sink)
    {
        ITypeSymbol? message;
        var typeArguments = TypeArgumentsOf(invocation.Expression);
        if (typeArguments is { Arguments.Count: > 0 })
        {
            message = model.GetTypeInfo(typeArguments.Arguments[0]).Type;
        }
        else if (invocation.ArgumentList.Arguments.Count == 0)
        {
            Unresolved++;
            return;
        }
        else
        {
            message = NaturalTypeOf(model, invocation.ArgumentList.Arguments[0].Expression);
        }

        if (message is { TypeKind: TypeKind.TypeParameter })
        {
            return;
        }

        if (message is null
            || message.TypeKind is TypeKind.Error or TypeKind.Dynamic
            || message.SpecialType == SpecialType.System_Object
            || message is INamedTypeSymbol { IsAnonymousType: true }
            || message.Name.Length == 0)
        {
            Unresolved++;
            return;
        }

        sink.Add(new FactRecord("publish", relFile, LineOf(invocation))
            .With("message", message.Name)
            .With("fqn", Fqn(message)));
    }

    private void EmitDiBinding(
        SemanticModel model, InvocationExpressionSyntax invocation, string relFile, List<FactRecord> sink)
    {
        var typeArguments = TypeArgumentsOf(invocation.Expression);
        if (typeArguments is null)
        {
            return;
        }

        var line = LineOf(invocation);
        if (typeArguments.Arguments.Count == 2)
        {
            var iface = model.GetTypeInfo(typeArguments.Arguments[0]).Type;
            var implementation = model.GetTypeInfo(typeArguments.Arguments[1]).Type;
            if (Resolved(iface) && Resolved(implementation))
            {
                sink.Add(Binding(relFile, line, iface, implementation));
            }
            else
            {
                Unresolved++;
            }

            return;
        }

        if (typeArguments.Arguments.Count != 1
            || invocation.ArgumentList.Arguments.FirstOrDefault()?.Expression
                is not AnonymousFunctionExpressionSyntax factory
            || factory is not (SimpleLambdaExpressionSyntax or ParenthesizedLambdaExpressionSyntax))
        {
            return;
        }

        var contract = model.GetTypeInfo(typeArguments.Arguments[0]).Type;
        var produced = LambdaResultType(model, factory);
        if (Resolved(contract) && Resolved(produced))
        {
            sink.Add(Binding(relFile, line, contract, produced));
        }
        else
        {
            Unresolved++;
        }
    }

    private static FactRecord Binding(string relFile, int line, ITypeSymbol iface, ITypeSymbol implementation) =>
        new FactRecord("di_binding", relFile, line)
            .With("iface", iface.Name)
            .With("impl", implementation.Name)
            .With("ifaceFqn", Fqn(iface))
            .With("implFqn", Fqn(implementation));

    private static bool Resolved([System.Diagnostics.CodeAnalysis.NotNullWhen(true)] ITypeSymbol? type) =>
        type is not null && type.TypeKind != TypeKind.Error;

    private static ITypeSymbol? LambdaResultType(SemanticModel model, AnonymousFunctionExpressionSyntax lambda)
    {
        if (lambda.Body is BlockSyntax block)
        {
            var last = Inside(block).OfType<ReturnStatementSyntax>().LastOrDefault();
            return last?.Expression is { } returned ? model.GetTypeInfo(returned).Type : null;
        }

        return lambda.Body is ExpressionSyntax expression ? model.GetTypeInfo(expression).Type : null;
    }

    /// <summary>Nodes of one body, not descending into nested lambdas or local functions.</summary>
    private static IEnumerable<SyntaxNode> Inside(SyntaxNode body) =>
        body.DescendantNodes(n => n is not (AnonymousFunctionExpressionSyntax or LocalFunctionStatementSyntax));

    private static string? InvokedName(SemanticModel model, InvocationExpressionSyntax invocation) =>
        model.GetSymbolInfo(invocation).Symbol is IMethodSymbol method
            ? method.Name
            : SyntacticName(invocation);

    /// <summary>The called member's name as written, with nothing bound.</summary>
    private static string? SyntacticName(InvocationExpressionSyntax invocation) => invocation.Expression switch
    {
        MemberAccessExpressionSyntax access => access.Name.Identifier.ValueText,
        MemberBindingExpressionSyntax binding => binding.Name.Identifier.ValueText,
        SimpleNameSyntax simple => simple.Identifier.ValueText,
        _ => null,
    };

    private static ExpressionSyntax? ReceiverOf(ExpressionSyntax callee) =>
        callee is MemberAccessExpressionSyntax access ? access.Expression : null;

    private static TypeArgumentListSyntax? TypeArgumentsOf(ExpressionSyntax callee) => callee switch
    {
        GenericNameSyntax generic => generic.TypeArgumentList,
        MemberAccessExpressionSyntax { Name: GenericNameSyntax generic } => generic.TypeArgumentList,
        MemberBindingExpressionSyntax { Name: GenericNameSyntax generic } => generic.TypeArgumentList,
        _ => null,
    };

    private static ITypeSymbol? NaturalTypeOf(SemanticModel model, ExpressionSyntax expression)
    {
        if (expression is not AwaitExpressionSyntax awaited)
        {
            return model.GetTypeInfo(expression).Type;
        }

        var operand = model.GetTypeInfo(awaited.Expression).Type;
        return operand is INamedTypeSymbol { Name: "Task" or "ValueTask", TypeArguments.Length: 1 } task
            ? task.TypeArguments[0]
            : operand;
    }

    // ---------------------------------------------------------------- shared

    private static string Fqn(ISymbol symbol) => symbol.ToDisplayString(FullyQualified);

    private static int LineOf(SyntaxNode node) => node.GetLocation().GetLineSpan().StartLinePosition.Line + 1;

    private static int LineOf(SyntaxToken token) => token.GetLocation().GetLineSpan().StartLinePosition.Line + 1;

    private static int EndLineOf(SyntaxNode node) => node.GetLocation().GetLineSpan().EndLinePosition.Line + 1;
}

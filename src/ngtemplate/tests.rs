use super::*;

fn reads(text: &str) -> Vec<(String, usize)> {
    scan_template(text, 1)
        .reads
        .into_iter()
        .map(|r| (r.name, r.line))
        .collect()
}

fn names(text: &str) -> Vec<String> {
    reads(text).into_iter().map(|(n, _)| n).collect()
}

fn gaps(text: &str) -> Vec<(usize, GapReason)> {
    scan_template(text, 1)
        .gaps
        .into_iter()
        .map(|g| (g.line, g.reason))
        .collect()
}

#[test]
fn angular_templates_ac_2_each_binding_form_reads_its_root() {
    let t = "<p *ngIf=\"shown\"></p>\n\
             <li *ngFor=\"let row of rows; trackBy: rowKey\"></li>\n\
             <b *appRepeat=\"times\"></b>\n\
             <i [title]=\"caption\"></i>\n\
             <i [attr.aria-label]=\"label\"></i>\n\
             <i [class.active]=\"active\"></i>\n\
             <i [style.width]=\"width\"></i>\n\
             <button (click)=\"save()\"></button>\n\
             <input [(ngModel)]=\"draft\">\n\
             <span>{{ greeting }}</span>\n\
             <img alt=\"{{ altText }}\">";
    assert_eq!(
        reads(t),
        [
            ("shown", 1),
            ("rows", 2),
            ("rowKey", 2),
            ("times", 3),
            ("caption", 4),
            ("label", 5),
            ("active", 6),
            ("width", 7),
            ("save", 8),
            ("draft", 9),
            ("greeting", 10),
            ("altText", 11),
        ]
        .map(|(n, l)| (n.to_string(), l))
    );
    assert!(gaps(t).is_empty());
}

#[test]
fn angular_templates_ac_2_roots_chains_calls_this_and_pipes() {
    assert_eq!(names("{{ user.profile.name }}"), ["user"]);
    assert_eq!(names("<a (click)=\"pick(item)\"></a>"), ["pick", "item"]);
    assert_eq!(names("{{ this.title }}"), ["title"]);
    assert_eq!(names("{{ amount | currency:code }}"), ["amount", "code"]);
    assert_eq!(names("{{ a?.b!.c[key] }}"), ["a", "key"]);
    assert_eq!(names("{{ ok ? yes : no }}"), ["ok", "yes", "no"]);
    assert_eq!(names("{{ x ?? -y }}"), ["x", "y"]);
    assert_eq!(
        names("{{ [one, {k: two, three}] }}"),
        ["one", "two", "three"]
    );
    assert_eq!(names("{{ $any(raw).v }}"), ["raw"]);
    assert_eq!(
        names("{{ true || null || undefined || 'text' || 3.5 }}"),
        Vec::<String>::new()
    );
}

#[test]
fn angular_templates_ac_2_an_assignment_target_is_a_read() {
    assert_eq!(
        names("<a (click)=\"picked = item; count += 1; done()\"></a>"),
        ["picked", "item", "count", "done"]
    );
}

#[test]
fn angular_templates_ac_2_the_line_is_the_identifier_line() {
    let t = "<a\n  [title]=\"first +\n    second\"\n  (click)=\"go()\">\n{{\n  later }}</a>";
    assert_eq!(
        reads(t),
        [("first", 2), ("second", 3), ("go", 4), ("later", 6)].map(|(n, l)| (n.to_string(), l))
    );
    let shifted = scan_template("{{ x }}\n{{ y }}", 10);
    assert_eq!(shifted.reads[1].line, 11);
}

#[test]
fn angular_templates_ac_4_locals_hide_reads_in_their_scope() {
    let t = "<input #query>\n\
             <ng-template let-entry>{{ entry }}</ng-template>\n\
             <li *ngFor=\"let item of items; index as i\">{{ item }} {{ i }}</li>\n\
             <p *ngIf=\"pending as job\">{{ job }}</p>\n\
             <button (click)=\"search(query.value, $event)\"></button>\n\
             {{ item }}";
    assert_eq!(
        reads(t),
        [("items", 3), ("pending", 4), ("search", 5), ("item", 6)].map(|(n, l)| (n.to_string(), l))
    );
}

#[test]
fn angular_templates_ac_4_a_ref_covers_the_whole_template() {
    assert_eq!(
        names("{{ box.value }}<input #box>{{ box }}"),
        Vec::<String>::new()
    );
}

#[test]
fn angular_templates_ac_4_event_only_hides_event_in_its_own_binding() {
    assert_eq!(
        names("<a (click)=\"f($event)\" [x]=\"$event\"></a>"),
        ["f", "$event"]
    );
}

#[test]
fn angular_templates_ac_4_structural_locals_end_with_their_element() {
    let t = "<div *ngFor=\"let row of rows\"><br>{{ row }}</div>{{ row }}";
    assert_eq!(names(t), ["rows", "row"]);
    let void = "<img *ngIf=\"src as s\" [src]=\"s\">{{ s }}";
    assert_eq!(names(void), ["src", "s"]);
}

#[test]
fn angular_templates_ac_4_a_stray_end_tag_keeps_the_open_scopes() {
    let t = "<div *ngFor=\"let row of rows\"></span>{{ row }}</div>{{ row }}";
    assert_eq!(names(t), ["rows", "row"]);
    let unclosed = "<section *ngIf=\"a as v\"><p>{{ v }}";
    assert_eq!(names(unclosed), ["a"]);
}

#[test]
fn angular_templates_ac_7_a_control_flow_line_is_a_gap_and_its_markup_is_scanned() {
    let t = "@if (ready) {\n<b>{{ shown }}</b>\n} @else {\n<i>{{ other }}</i>\n}\nmail@if.example";
    assert_eq!(
        gaps(t),
        [
            (1, GapReason::ControlFlowBlock),
            (3, GapReason::ControlFlowBlock)
        ]
    );
    assert_eq!(names(t), ["shown", "other"]);
}

#[test]
fn angular_templates_ac_7_an_unparsable_expression_is_one_gap() {
    let t = "<a [x]=\"a &amp;&amp; b\"></a>\n{{ `tpl` }}\n<a (click)=\"f(\"></a>\n{{ ok }}";
    assert_eq!(
        gaps(t),
        [
            (1, GapReason::UnparsedExpression),
            (2, GapReason::UnparsedExpression),
            (3, GapReason::UnparsedExpression),
        ]
    );
    assert_eq!(names(t), ["ok"]);
}

#[test]
fn angular_templates_ac_7_a_deep_expression_is_a_gap_not_a_crash() {
    let deep = format!("{{{{ {}x{} }}}}", "(".repeat(5000), ")".repeat(5000));
    assert_eq!(gaps(&deep), [(1, GapReason::UnparsedExpression)]);
    let prefix = format!("{{{{ {}x }}}}", "!".repeat(5000));
    assert_eq!(gaps(&prefix), [(1, GapReason::UnparsedExpression)]);
    let chain = format!("<a (click)=\"{}1\"></a>", "a = ".repeat(5000));
    assert_eq!(names(&chain), ["a"; 5000]);
}

#[test]
fn angular_templates_ac_7_an_unterminated_value_ends_the_scan_with_one_gap() {
    let t = "<a [x]=\"one\"></a>\n<a [title]=\"two\n{{ three }}";
    assert_eq!(gaps(t), [(2, GapReason::UnparsedExpression)]);
    assert_eq!(names(t), ["one"]);
    assert_eq!(gaps("{{ never"), [(1, GapReason::UnparsedExpression)]);
}

#[test]
fn angular_templates_ac_7_comments_scripts_and_styles_give_nothing() {
    let t = "<!-- {{ hidden }} -->\n<script>{{ code }}</script><style>a{{b}}</style>\n{{ seen }}";
    assert_eq!(reads(t), [("seen".to_string(), 3)]);
    assert!(gaps(t).is_empty());
}

#[test]
fn angular_templates_ac_2_a_microsyntax_value_without_separators() {
    assert_eq!(
        names("<li *ngFor=\"let a of list trackBy: key; let n = index\">{{ n }}</li>"),
        ["list", "key"]
    );
    assert_eq!(
        names("<p *ngIf=\"busy; else spinner\"></p><ng-template #spinner></ng-template>"),
        ["busy"]
    );
    assert_eq!(
        names("<p *ngIf=\"stream | async as v\">{{ v }}</p>"),
        ["stream"]
    );
}

#[test]
fn angular_templates_ac_2_text_that_only_looks_like_markup() {
    assert_eq!(names("a < b {{ c }} <3 d"), ["c"]);
    assert_eq!(
        names("<!DOCTYPE html><p title=\"{{ t }}\" hidden></p>"),
        ["t"]
    );
}

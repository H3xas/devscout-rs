# angular-templates

A small Angular application whose components read their members from external
and inline templates. `map` turns each member into a name row and each template
read into a `uses-member` edge, and `refs` lists the template lines. The names
are invented. Nothing here is meant to build.

## Components

| File | Class | Template |
| --- | --- | --- |
| `src/app/profile-card.component.ts` | `ProfileCard` | `./profile-card.component.html` |
| `src/app/motto-banner.component.ts` | `MottoBanner` | `motto-banner.component.html`, a bare path |
| `src/app/widgets/badge-widget.component.ts` | `BadgeWidget` | `../shared/badge-view.html` |
| `src/app/inline-notes.component.ts` | `SingleNote`, `DoubleNote`, `BacktickNote` | inline, one for each quote form |
| `src/app/search-list.component.ts` | `SearchList` | `./search-list.component.html` |
| `src/app/panels/*.component.ts` | `AlphaPanel`, `BetaPanel`, `GammaPanel` | one template each |
| `src/app/gap-showcase.component.ts` | `GapShowcase` | `./gap-showcase.component.html` |
| `src/app/template-sources.component.ts` | `MissingTemplate`, `ComputedTemplate`, `SubstitutedTemplate` | a missing file, a constant, a substitution |
| `src/app/member-kinds.component.ts` | `MemberKinds` | inline |

`src/app/plain-helper.ts` declares `PlainHelper`, a class without a decorator, so
its member gets no name row. `src/app/showcase-base.ts` declares the base class of
`GapShowcase`.

## Template forms

`profile-card.component.html` reads one member on each line, except its last
line, which reads two. The structural lines are `*ngIf`, `*ngFor` and one more
directive. The `*ngFor` puts its `of` expression and its `trackBy` on two lines.
The binding lines are `[property]`, `[attr.x]`, `[class.x]`, `[style.x]`,
`(event)` and `[(twoWay)]`. Then come interpolation in text and in an attribute
value, a member chain, `this.`, a pipe with an argument, and a call with an
argument. The class also declares
`unusedFlag`, which no template reads.

`search-list.component.html` declares a `#ref`, a `let-x` on `ng-template`, an
`*ngFor` item with `index as`, an `*ngIf` with `as`, and `$event`. Each local has
the name of a field of the class. Only the last line reads the field `item`
outside the local's scope.

The three panels declare a field with the same name. The first two templates
read it, and the third template reads another field.

## Sites without an edge

`gap-showcase.component.html` opens a control flow block. The markup inside the
block still gives an edge. The template also holds an expression with an HTML
entity that the scanner cannot parse, and it reads a member of the base class.
`template-sources.component.ts` names a template that does not exist, takes its
`templateUrl` from a constant, and builds its `template` with a substitution.
`member-kinds.component.ts` reads a static member.

`orphan-view.html` holds reads, an expression that the scanner cannot parse and
a control flow block, but no component names it, so it gives nothing.

# csharp-next-hints fixture

One C# tree whose answers are larger than the caps they render under, so the
count line and the `next:` trailer have something to describe. Every file is
plain C# with no external reference beyond `Xunit` on the test file.

| File | What it pins |
| --- | --- |
| `IPriceSource.cs` | The wide-inbound symbol. 55 reader files take it as a parameter and call its member, and one class derives from it: 111 inbound rows across `inherits`, `uses-type` and `uses-member`, against an inbound cap of 30. |
| `CachePriceSource.cs` | The single implementor, and the only `inherits` row on the interface. |
| `PriceSourceTests.cs` | The one test file reaching the interface, so `tests` answers with a row rather than the empty case. |
| `UnusedReport.cs` | A type and a member nothing references: `refs UnusedReport.Total` is the zero-hit case (`0 inbound (showing 0)`), and `impact UnusedReport.cs` is the empty-blast-radius case (`0 files (showing 0)`). |
| `Resettable.cs` | One member name (`Reset`) two types declare and nothing references, so the seed is refused as ambiguous rather than guessed between them. |
| `Readers/Reader01.cs` … `Readers/Reader55.cs` | One file per referencing class, so the interface's blast radius is 56 files against an impact cap of 50, and its inbound table overflows the 30-row cap. |

The counts matter to the tests rather than to the code: a fixture below either
cap would render a complete table and prove nothing about a count line.

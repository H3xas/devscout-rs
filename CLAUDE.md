# Comments

Code shows how; a comment earns its place only by carrying a *why* the
code can't. Test intent belongs in the test itself or the fixture
README. A hook and CI reject "Case X:" notes, file:line citations,
tracker ids, tool/model names, plan labels ("Unit A3 item 4", bare
"item 7"), and test narration ("probes", "exercises the") in comments
and commit messages.

Bad:  `// Case a: Widget shares a name with Gadget -- probes bare-name lookup`
Good: `// Bare-name lookup must skip nested types to avoid Widget/Gadget collisions`
Scan: `python3 .claude/hooks/comment-hygiene.py --scan`
Scan commits: `python3 .claude/hooks/comment-hygiene.py --scan-commits origin/main..HEAD`
Selfcheck: `python3 .claude/hooks/comment-hygiene.py --selfcheck`

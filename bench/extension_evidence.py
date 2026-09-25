"""Semantic-audit joins and graph-only extension candidate selection."""

from collections import Counter, defaultdict


def tier(edge):
    value = edge.get("tier")
    if value in ("ext", "guess"):
        return value
    return "heuristic" if edge.get("heuristic") else "precise"


def site(edge):
    return edge.get("from_file", ""), edge.get("from_line", 0)


def target_matches(record, target):
    return record.get("target") == target or (
        record.get("targetKind") == "enum-member"
        and (record.get("target") or "").rpartition(".")[0] == target
    )


def matches(record, edge):
    return (
        not record.get("external")
        and target_matches(record, edge.get("to"))
        and (edge.get("member") is None or edge["member"] == record.get("member"))
    )


def target_id(record, definitions):
    target = record.get("target")
    if target in definitions:
        return target
    if record.get("targetKind") == "enum-member" and target:
        parent = target.rpartition(".")[0]
        if parent in definitions:
            return parent
    return None


def ratio(numerator, denominator):
    return numerator / denominator if denominator else None


def candidates(graph):
    definitions = defaultdict(list)
    sites = Counter()
    for definition in graph["defs"]:
        definitions[definition["id"]].append(definition)
    for edge in graph["edges"]:
        if edge["kind"] == "uses-member":
            sites[(*site(edge), edge.get("member"))] += 1
    accepted, rejected = [], Counter()
    for number, edge in enumerate(graph["edges"]):
        if edge["kind"] != "uses-member" or tier(edge) != "ext":
            continue
        definitions_at_target = definitions[edge.get("to")]
        if not edge.get("heuristic") or not edge.get("member") or site(edge)[1] <= 0:
            reason = "missing_evidence"
        elif sites[(*site(edge), edge.get("member"))] != 1:
            reason = "competing_site"
        elif len(definitions_at_target) != 1:
            reason = "target_identity"
        elif definitions_at_target[0]["file"] != edge.get("to_file"):
            reason = "target_file"
        elif edge["member"] not in definitions_at_target[0].get("methods", []):
            reason = "undeclared_method"
        else:
            accepted.append(number)
            continue
        rejected[reason] += 1
    return accepted, dict(rejected)


class Evidence:
    def __init__(self, graph, records, units, manifest=None):
        mapped = set((manifest or {}).get("entries", {}))
        if not mapped:
            mapped = {d["file"] for d in graph["defs"]}
            for edge in graph["edges"]:
                mapped.update(edge[key] for key in ("from_file", "to_file") if key in edge)
        compiled = {f for u in units if u["status"] == "ok" for f in u["files"]}
        self.universe = mapped & compiled if units else mapped
        self.graph = graph
        self.definitions = {d["id"]: d for d in graph["defs"]}
        self.records = [r for r in records if r["file"] in self.universe]
        self.by_site = defaultdict(list)
        for record in self.records:
            self.by_site[record["file"], record["startLine"]].append(record)
        self.edges = {
            i: e for i, e in enumerate(graph["edges"])
            if e["kind"] == "uses-member" and (not units or e["from_file"] in self.universe)
        }
        self.denominator = [
            r for r in self.records
            if r["shape"] == "access" and not r["external"]
            and target_id(r, self.definitions) is not None
        ]

    def hit_records(self, numbers):
        by_site = defaultdict(list)
        for number in numbers:
            edge = self.edges[number]
            by_site[site(edge)].append(edge)
        return {
            i for i, record in enumerate(self.denominator)
            if any(matches(record, e) for e in by_site[record["file"], record["startLine"]])
        }

    def stats(self, numbers):
        numbers = set(numbers)
        tp = sum(
            any(matches(r, self.edges[i]) for r in self.by_site[site(self.edges[i])])
            for i in numbers
        )
        hits = self.hit_records(numbers)
        return dict(edges=len(numbers), tp=tp, fp=len(numbers)-tp,
                    precision=ratio(tp, len(numbers)), recall_hits=len(hits),
                    recall_denominator=len(self.denominator),
                    recall=ratio(len(hits), len(self.denominator)))

    def cohort(self):
        accepted, rejected = candidates(self.graph)
        selected = set(accepted) & self.edges.keys()
        precise = {i for i, e in self.edges.items() if tier(e) == "precise"}
        ext = {i for i, e in self.edges.items() if tier(e) == "ext"}
        ext_hits, precise_hits = self.hit_records(ext), self.hit_records(precise)
        result = {name: self.stats(numbers) for name, numbers in (
            ("precise", precise), ("ext", ext), ("candidate", selected),
            ("combined", precise | selected),
            ("guess", {i for i, e in self.edges.items() if tier(e) == "guess"}),
        )}
        result.update(candidate_rejections=rejected, candidate_numbers=sorted(selected),
                      ext_overlap_with_precise=len(ext_hits & precise_hits),
                      ext_only_recall_hits=len(ext_hits - precise_hits),
                      candidate_only_recall_hits=len(self.hit_records(selected)-precise_hits),
                      covered_files=len(self.universe),
                      unjudged_member_edges=sum(e["kind"] == "uses-member" for e in self.graph["edges"])-len(self.edges))
        return result

    def replay_graphs(self):
        # Non-member context is shared, so this oracle only judges member-reference reachability.
        graph = dict(self.graph)
        graph["defs"] = [d for d in graph["defs"] if d["file"] in self.universe]
        graph["edges"] = [e for e in graph["edges"] if e.get("from_file", e.get("file")) in self.universe]
        oracle = dict(graph)
        oracle["edges"] = [e for e in graph["edges"] if e["kind"] != "uses-member"
                           and not (e["kind"] == "ambiguous" and e.get("origin") == "uses-member")]
        seen = set()
        for record in self.records:
            target = target_id(record, self.definitions)
            if record["external"] or target is None:
                continue
            target_file = self.definitions[target]["file"]
            if target_file not in self.universe:
                continue
            key = record["file"], record["startLine"], target, record["member"]
            if key in seen:
                continue
            seen.add(key)
            oracle["edges"].append(dict(kind="uses-member", from_file=key[0], from_line=key[1],
                                        to=target, to_file=target_file, member=key[3]))
        return graph, oracle


def answer_stats(rows, truth):
    files = {row["file"] for row in rows}
    return dict(rows=len(files), tp=len(files & truth), fp=len(files-truth),
                precision=ratio(len(files & truth), len(files)),
                recall=ratio(len(files & truth), len(truth)))


def summarize(answers):
    counts = {name: Counter() for name in ("baseline", "candidate", "added", "displaced",
                                          "baseline_asserted", "candidate_asserted")}
    per_answer = []
    for answer in answers:
        truth = {r["file"] for r in answer["oracle"]["rows"]}
        before, after = answer["baseline"]["rows"], answer["candidate"]["rows"]
        before_files, after_files = {r["file"] for r in before}, {r["file"] for r in after}
        cohorts = dict(baseline=before, candidate=after,
                       added=[r for r in after if r["file"] not in before_files],
                       displaced=[r for r in before if r["file"] not in after_files],
                       baseline_asserted=[r for r in before if not r["heuristic"]],
                       candidate_asserted=[r for r in after if not r["heuristic"]])
        detail = dict(seed=answer["seed"], truth_rows=len(truth))
        for name, rows in cohorts.items():
            stats = answer_stats(rows, truth)
            detail[name] = stats
            counts[name].update({k: stats[k] for k in ("rows", "tp", "fp")})
            counts[name]["truth_rows"] += len(truth)
        per_answer.append(detail)
    for name, stats in counts.items():
        stats["precision"] = ratio(stats["tp"], stats["rows"])
        stats["recall"] = ratio(stats["tp"], stats["truth_rows"])
        defined = [r[name]["precision"] for r in per_answer if r[name]["precision"] is not None]
        stats["macro_precision"] = ratio(sum(defined), len(defined))
        stats["worst_precision"] = min(defined) if defined else None
        stats["below_floor_answers"] = sum(x < 0.95 for x in defined)
        stats["undefined_answers"] = len(answers)-len(defined)
    return dict(queries=len(answers), cohorts=counts, per_answer=per_answer)


def admission_failures(cohort, answers):
    failures = []
    for name in ("candidate", "combined"):
        stats = cohort[name]
        if not stats["edges"] or stats["tp"] * 100 < stats["edges"] * 95:
            failures.append(name + " edge precision is undefined or below 0.95")
    before = answers["cohorts"]["baseline"]
    after = answers["cohorts"]["candidate"]
    if not after["rows"] or after["tp"] * 100 < after["rows"] * 95:
        failures.append("whole-answer precision is undefined or below 0.95")
    if before["rows"] and after["tp"] * before["rows"] < before["tp"] * after["rows"]:
        failures.append("whole-answer precision decreases")
    if after["below_floor_answers"]:
        failures.append("individual candidate answers fall below 0.95")
    asserted_before = answers["cohorts"].get("baseline_asserted")
    asserted_after = answers["cohorts"].get("candidate_asserted")
    if asserted_before and asserted_after:
        if (asserted_before["rows"] and asserted_after["rows"]
                and asserted_after["tp"] * asserted_before["rows"]
                < asserted_before["tp"] * asserted_after["rows"]):
            failures.append("asserted affected-answer precision decreases")
        if asserted_after["below_floor_answers"]:
            failures.append("individual asserted affected answers fall below 0.95")
    added = answers["cohorts"]["added"]
    if added["rows"] and added["tp"] * 100 < added["rows"] * 95:
        failures.append("newly shown row precision is below 0.95")
    return failures

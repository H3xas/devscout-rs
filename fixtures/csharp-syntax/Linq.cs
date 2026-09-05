// References: query-expression clauses (from/where/orderby/group/join/let/select/into) and their method-syntax equivalents.
namespace Syntax.Query;

class QueryOwner
{
    public int Id;
    public string? Name;
}

class QueryItem
{
    public int Value;
    public QueryOwner Owner = new();
}

class QueryUser
{
    public void Run(List<QueryItem> items, List<QueryOwner> owners)
    {
        var basic =
            from i in items
            where i.Value > 0
            orderby i.Value descending, i.Owner.Id
            select i.Owner.Name;

        var grouped =
            from i in items
            group i by i.Owner.Id into g
            select g.Key;

        var joined =
            from i in items
            join o in owners on i.Owner.Id equals o.Id
            select o.Name;

        var groupJoined =
            from i in items
            join o in owners on i.Owner.Id equals o.Id into grp
            from o2 in grp.DefaultIfEmpty()
            select o2 != null ? o2.Name : null;

        var withLet =
            from i in items
            let total = i.Value * 2
            select total;

        var nested =
            from i in items
            from c in i.Owner.Name!
            select c;

        var explicitType =
            from QueryItem i in items
            select i.Owner;

        var continued =
            from i in items
            select i into j
            where j.Value > 1
            select j;

        var projected =
            from i in items
            select new { i.Value, i.Owner.Name };

        var m1 = items.Where(i => i.Value > 0).Select(i => i.Owner.Name).ToList();
        var m2 = items.GroupBy(i => i.Owner.Id);
        var m3 = items.Join(owners, i => i.Owner.Id, o => o.Id, (i, o) => o.Name);

        _ = basic;
        _ = grouped;
        _ = joined;
        _ = groupJoined;
        _ = withLet;
        _ = nested;
        _ = explicitType;
        _ = continued;
        _ = projected;
        _ = m1;
        _ = m2;
        _ = m3;
    }
}

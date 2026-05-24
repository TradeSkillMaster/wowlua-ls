use super::{AnnotationType, ParamInfo, TuplePosition};

pub(crate) fn format_annotation_type(at: &AnnotationType) -> String {
    match at {
        AnnotationType::Simple(s) => s.clone(),
        AnnotationType::Array(inner) => format!("{}[]", format_annotation_type(inner)),
        AnnotationType::Union(types) if types.len() == 2
            && types.iter().any(|t| matches!(t, AnnotationType::Simple(s) if s == "nil"))
            && types.iter().any(|t| !matches!(t, AnnotationType::Simple(s) if s == "nil")) =>
        {
            let other = types.iter()
                .find(|t| !matches!(t, AnnotationType::Simple(s) if s == "nil"))
                .unwrap();
            let formatted = format_annotation_type(other);
            if matches!(other, AnnotationType::Fun(..)) {
                format!("({})?", formatted)
            } else {
                format!("{}?", formatted)
            }
        }
        AnnotationType::Union(types) => types.iter()
            .map(format_annotation_type)
            .collect::<Vec<_>>()
            .join(" | "),
        AnnotationType::Parameterized(name, params) => {
            let params_str = params.iter()
                .map(format_annotation_type)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{}>", name, params_str)
        }
        AnnotationType::Backtick(inner) => format_annotation_type(inner),
        AnnotationType::NonNil(inner) => format!("{}!", format_annotation_type(inner)),
        AnnotationType::Intersection(types) => {
            let formatted = types.iter()
                .map(format_annotation_type)
                .collect::<Vec<_>>()
                .join(" & ");
            // Single-element intersection from `& ...M` syntax — preserve the
            // leading `&` so it round-trips correctly.
            if types.len() == 1 && matches!(&types[0], AnnotationType::VarArgs(_)) {
                format!("& {}", formatted)
            } else {
                formatted
            }
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            let mut args: Vec<String> = params.iter().map(|p| {
                let suffix = if p.optional { "?" } else { "" };
                format!("{}{}: {}", p.name, suffix, format_annotation_type(&p.typ))
            }).collect();
            if *is_vararg { args.push("...".to_string()); }
            let ret_str = if returns.is_empty() {
                String::new()
            } else {
                format!(": {}", returns.iter().map(format_annotation_type).collect::<Vec<_>>().join(", "))
            };
            format!("fun({}){}", args.join(", "), ret_str)
        }
        AnnotationType::TableLiteral(fields) => {
            let parts: Vec<String> = fields.iter().map(|(name, typ)| {
                format!("{}: {}", name, format_annotation_type(typ))
            }).collect();
            format!("{{{}}}", parts.join(", "))
        }
        AnnotationType::VarArgs(inner) => {
            format!("...{}", format_annotation_type(inner))
        }
        AnnotationType::Tuple(positions, description) => {
            let parts: Vec<String> = positions.iter().map(|p| {
                match &p.name {
                    Some(n) => format!("{} {}", format_annotation_type(&p.typ), n),
                    None => format_annotation_type(&p.typ),
                }
            }).collect();
            match description {
                Some(d) => format!("({}) {}", parts.join(", "), d),
                None => format!("({})", parts.join(", ")),
            }
        }
    }
}

pub(crate) fn substitute_alias_type_params(
    body: &AnnotationType,
    type_params: &[String],
    args: &[AnnotationType],
) -> AnnotationType {
    match body {
        AnnotationType::Simple(name) => {
            if let Some(pos) = type_params.iter().position(|p| p == name) {
                args[pos].clone()
            } else {
                body.clone()
            }
        }
        AnnotationType::Union(parts) => {
            AnnotationType::Union(parts.iter().map(|p| substitute_alias_type_params(p, type_params, args)).collect())
        }
        AnnotationType::Array(inner) => {
            AnnotationType::Array(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Parameterized(base, pargs) => {
            AnnotationType::Parameterized(
                base.clone(),
                pargs.iter().map(|a| substitute_alias_type_params(a, type_params, args)).collect(),
            )
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            let new_params = params.iter().map(|p| ParamInfo {
                name: p.name.clone(),
                typ: substitute_alias_type_params(&p.typ, type_params, args),
                optional: p.optional,
                description: p.description.clone(),
            }).collect();
            let new_returns = returns.iter().map(|r| substitute_alias_type_params(r, type_params, args)).collect();
            AnnotationType::Fun(new_params, new_returns, *is_vararg)
        }
        AnnotationType::NonNil(inner) => {
            AnnotationType::NonNil(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Intersection(parts) => {
            AnnotationType::Intersection(parts.iter().map(|p| substitute_alias_type_params(p, type_params, args)).collect())
        }
        AnnotationType::TableLiteral(fields) => {
            AnnotationType::TableLiteral(fields.iter().map(|(n, t)| {
                (n.clone(), substitute_alias_type_params(t, type_params, args))
            }).collect())
        }
        AnnotationType::Backtick(inner) => {
            AnnotationType::Backtick(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::VarArgs(inner) => {
            AnnotationType::VarArgs(Box::new(substitute_alias_type_params(inner, type_params, args)))
        }
        AnnotationType::Tuple(positions, description) => {
            AnnotationType::Tuple(
                positions.iter().map(|p| TuplePosition {
                    typ: substitute_alias_type_params(&p.typ, type_params, args),
                    name: p.name.clone(),
                }).collect(),
                description.clone(),
            )
        }
    }
}

pub(crate) fn match_projection(
    at: &AnnotationType,
    generic_names: &[String],
) -> Option<crate::types::ProjectionKind> {
    if let AnnotationType::Parameterized(base, args) = at {
        if args.is_empty() || args.len() > 2 { return None; }
        let name = match &args[0] {
            AnnotationType::Simple(n) if generic_names.iter().any(|g| g == n) => n.clone(),
            _ => return None,
        };
        return match base.as_str() {
            "params" if args.len() == 1 => Some(crate::types::ProjectionKind::Params(name)),
            "returns" => {
                let offset_param = if args.len() == 2 {
                    // Second arg is a parameter name (not a generic), accept any Simple name
                    match &args[1] {
                        AnnotationType::Simple(n) => Some(n.clone()),
                        _ => return None,
                    }
                } else {
                    None
                };
                Some(crate::types::ProjectionKind::Return(name, offset_param))
            }
            _ => None,
        };
    }
    // Check inside unions: `string | returns<F>` should detect the projection.
    // The non-projection members remain in the resolved annotation type and
    // get unioned with the projected type at call sites.
    if let AnnotationType::Union(parts) = at {
        for part in parts {
            if let Some(proj) = match_projection(part, generic_names) {
                return Some(proj);
            }
        }
    }
    None
}

pub(crate) fn parse_type(s: &str) -> AnnotationType {
    let s = s.trim();
    if s.is_empty() { return AnnotationType::Simple(s.to_string()); }
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        return AnnotationType::Backtick(Box::new(parse_type(&s[1..s.len()-1])));
    }
    // `& ...M` — intersection-of-varargs: a single value that is the intersection
    // of all types collected by the variadic generic `...M`.  Parses as
    // `Intersection([VarArgs(M)])` so the VarArgs expands inside the intersection
    // during generic substitution, just like `T & ...M`.
    if let Some(rest) = s.strip_prefix('&') {
        let rest = rest.trim();
        if rest.starts_with("...") {
            let inner = parse_type(rest);
            return AnnotationType::Intersection(vec![inner]);
        }
    }
    // Handle `...` prefix before `!`/`?` suffixes so that `...T?` parses as
    // `VarArgs(T?)` rather than `Union(VarArgs(T), nil)`.  This keeps the
    // outer node as `VarArgs`, which is required for `has_vararg_return` detection.
    if let Some(inner) = s.strip_prefix("...") {
        let inner_type = if inner.is_empty() {
            AnnotationType::Simple("any".to_string())
        } else {
            parse_type(inner)
        };
        return AnnotationType::VarArgs(Box::new(inner_type));
    }
    if let Some(without_bang) = s.strip_suffix('!') {
        let mut depth = 0usize;
        let is_fun_type = without_bang.starts_with("fun(") || without_bang.starts_with("async fun(");
        let mut found_return_colon = false;
        for c in without_bang.chars() {
            match c {
                '<' | '(' => depth += 1,
                '>' | ')' => depth = depth.saturating_sub(1),
                ':' if depth == 0 && is_fun_type => found_return_colon = true,
                _ => {}
            }
        }
        if depth == 0 && !found_return_colon {
            let base_type = parse_type(without_bang);
            return AnnotationType::NonNil(Box::new(base_type));
        }
    }
    if let Some(without_q) = s.strip_suffix('?') {
        let mut depth = 0usize;
        let is_fun_type = without_q.starts_with("fun(") || without_q.starts_with("async fun(");
        let mut found_return_colon = false;
        for c in without_q.chars() {
            match c {
                '<' | '(' => depth += 1,
                '>' | ')' => depth = depth.saturating_sub(1),
                ':' if depth == 0 && is_fun_type => found_return_colon = true,
                _ => {}
            }
        }
        if depth == 0 && !found_return_colon {
            let base_type = if without_q.is_empty() {
                AnnotationType::Simple("any".to_string())
            } else {
                parse_type(without_q)
            };
            return AnnotationType::Union(vec![base_type, AnnotationType::Simple("nil".to_string())]);
        }
    }
    let union_parts = split_at_top_level(s, '|');
    if union_parts.len() > 1 {
        let parts: Vec<AnnotationType> = union_parts.iter().map(|p| parse_type(p.trim())).collect();
        return AnnotationType::Union(parts);
    }
    let intersection_parts = split_at_top_level(s, '&');
    if intersection_parts.len() > 1 {
        let parts: Vec<AnnotationType> = intersection_parts.iter().map(|p| parse_type(p.trim())).collect();
        return AnnotationType::Intersection(parts);
    }
    if s.starts_with('(') {
        let mut depth = 0i32;
        let mut close = None;
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => { depth -= 1; if depth == 0 { close = Some(i); break; } }
                _ => {}
            }
        }
        if close == Some(s.len() - 1) {
            let inner = &s[1..s.len() - 1];
            let parts = split_at_top_level(inner, ',');
            if parts.len() > 1 {
                return AnnotationType::Tuple(parse_tuple_positions(&parts), None);
            }
            return parse_type(inner);
        }
    }
    let fun_str = s.strip_prefix("async ").unwrap_or(s);
    if fun_str.starts_with("fun(")
        && let Some(sig) = parse_overload(fun_str) {
            return AnnotationType::Fun(sig.params, sig.returns, sig.is_vararg);
        }
    if let Some(without_brackets) = s.strip_suffix("[]") {
        let base = parse_type(without_brackets);
        return AnnotationType::Array(Box::new(base));
    }
    if s.ends_with('>')
        && let Some(lt_pos) = s.find('<') {
            let base = s[..lt_pos].trim();
            let args_str = &s[lt_pos+1..s.len()-1];
            let args = split_at_top_level(args_str, ',');
            let arg_types: Vec<AnnotationType> = args.iter().map(|a| parse_type(a.trim())).collect();
            return AnnotationType::Parameterized(base.to_string(), arg_types);
        }
    if s.starts_with('{') && s.ends_with('}') {
        let inner = s[1..s.len()-1].trim();
        if inner.is_empty() {
            return AnnotationType::Simple("table".to_string());
        }
        let field_parts = split_at_top_level(inner, ',');
        let mut fields = Vec::new();
        let mut indexed_key: Option<(AnnotationType, AnnotationType)> = None;
        for part in &field_parts {
            let part = part.trim();
            if part.is_empty() { continue; }
            if part.starts_with('[')
                && let Some(bracket_end) = find_matching_bracket(part)
            {
                let after = part[bracket_end+1..].trim();
                if let Some(rest) = after.strip_prefix(':') {
                    let key_str = part[1..bracket_end].trim();
                    let val_str = rest.trim();
                    if !key_str.is_empty() && !val_str.is_empty() {
                        // Integer literal keys like [1], [2] → named fields "[1]", "[2]"
                        // matching extract_bracket_literal_key format for bracket access.
                        if key_str.bytes().all(|b| b.is_ascii_digit()) {
                            let field_name = format!("[{}]", key_str);
                            let field_type = parse_type(val_str);
                            fields.push((field_name, field_type));
                        } else if indexed_key.is_none() {
                            indexed_key = Some((parse_type(key_str), parse_type(val_str)));
                        }
                    }
                    continue;
                }
            }
            if let Some(colon_pos) = part.find(':') {
                let name = part[..colon_pos].trim();
                let type_str = part[colon_pos+1..].trim();
                let (name, optional) = if let Some(stripped) = name.strip_suffix('?') {
                    (stripped, true)
                } else {
                    (name, false)
                };
                if !name.is_empty() && !type_str.is_empty() {
                    let mut field_type = parse_type(type_str);
                    if optional {
                        field_type = AnnotationType::Union(vec![field_type, AnnotationType::Simple("nil".to_string())]);
                    }
                    fields.push((name.to_string(), field_type));
                }
            }
        }
        match (indexed_key, fields.is_empty()) {
            (Some((k, v)), true) => {
                return AnnotationType::Parameterized("table".to_string(), vec![k, v]);
            }
            (Some((k, v)), false) => {
                return AnnotationType::Intersection(vec![
                    AnnotationType::Parameterized("table".to_string(), vec![k, v]),
                    AnnotationType::TableLiteral(fields),
                ]);
            }
            (None, false) => {
                return AnnotationType::TableLiteral(fields);
            }
            (None, true) => {
                return AnnotationType::Simple("table".to_string());
            }
        }
    }
    if let Some(inner) = s.strip_prefix("...")
        && !inner.is_empty() {
            return parse_type(inner);
        }
    AnnotationType::Simple(s.to_string())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverloadSig {
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    pub is_vararg: bool,
    pub is_return_only: bool,
}

pub(crate) fn parse_overload(s: &str) -> Option<OverloadSig> {
    let s = s.trim();
    let rest = s.strip_prefix("fun(")?;
    let mut depth = 1u32;
    let mut close = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { close = Some(i); break; } }
            _ => {}
        }
    }
    let close = close?;
    let params_str = &rest[..close];
    let after_paren = rest[close + 1..].trim();

    let mut params = Vec::new();
    let mut is_vararg = false;
    if !params_str.is_empty() {
        for part in split_params(params_str) {
            let part = part.trim();
            if part == "..." {
                is_vararg = true;
                continue;
            }
            if let Some(vararg_type_str) = part.strip_prefix("...:").or_else(|| part.strip_prefix("...")) {
                is_vararg = true;
                let vararg_type_str = vararg_type_str.trim();
                if !vararg_type_str.is_empty() {
                    let ann_type = parse_type(vararg_type_str);
                    params.push(ParamInfo { name: "...".to_string(), typ: ann_type, optional: false, description: None });
                }
                continue;
            }
            if let Some((name, type_str)) = part.split_once(':') {
                let trimmed = name.trim();
                let optional = trimmed.ends_with('?');
                let name = trimmed.trim_end_matches('?').to_string();
                let ann_type = parse_type(type_str.trim());
                params.push(ParamInfo { name, typ: ann_type, optional, description: None });
            } else {
                let optional = part.ends_with('?');
                params.push(ParamInfo {
                    name: part.trim_end_matches('?').to_string(),
                    typ: AnnotationType::Simple("any".to_string()),
                    optional,
                    description: None,
                });
            }
        }
    }

    let returns = if let Some(ret_str) = after_paren.strip_prefix(':') {
        let ret_str = ret_str.trim();
        if ret_str.is_empty() { Vec::new() }
        else { split_params(ret_str).iter().map(|r| parse_type(r.trim())).collect() }
    } else { Vec::new() };

    Some(OverloadSig { params, returns, is_vararg, is_return_only: false })
}

fn parse_tuple_positions(parts: &[&str]) -> Vec<TuplePosition> {
    parts.iter().filter_map(|part| {
        let part = part.trim();
        if part.is_empty() { return None; }
        let type_text = extract_type_prefix(part);
        let name = extract_trailing_ident(part[type_text.len()..].trim());
        Some(TuplePosition { typ: parse_type(type_text), name })
    }).collect()
}

pub(crate) fn parse_return_line(s: &str, force_tuple: bool) -> (AnnotationType, Option<String>, Option<String>) {
    let s = s.trim();
    if s.starts_with('(') {
        let mut cases: Vec<(Vec<TuplePosition>, Option<String>)> = Vec::new();
        let mut first_trailing: Option<&str> = None;
        let mut rem = s;
        loop {
            if !rem.starts_with('(') { break; }
            let mut depth = 0i32;
            let mut close_idx = None;
            for (i, c) in rem.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => { depth -= 1; if depth == 0 { close_idx = Some(i); break; } }
                    _ => {}
                }
            }
            let Some(end) = close_idx else { break; };
            let inner = &rem[1..end];
            let after = rem[end + 1..].trim_start();
            let parts = split_at_top_level(inner, ',');
            let positions = parse_tuple_positions(&parts);
            let (case_trailing, next_rem) = {
                let mut split = None;
                let bytes = after.as_bytes();
                for (i, &b) in bytes.iter().enumerate() {
                    if b == b'|' {
                        let rest = after[i + 1..].trim_start();
                        if rest.starts_with('(') { split = Some((i, rest)); break; }
                    }
                }
                match split {
                    Some((i, next)) => (after[..i].trim(), Some(next)),
                    None => (after, None),
                }
            };
            if cases.is_empty() { first_trailing = Some(after); }
            let desc = {
                let t = case_trailing.strip_prefix('@').unwrap_or(case_trailing).trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            };
            cases.push((positions, desc));
            match next_rem {
                Some(next) => rem = next,
                None => break,
            }
        }
        if cases.len() >= 2 {
            let members: Vec<AnnotationType> = cases.into_iter()
                .map(|(p, d)| AnnotationType::Tuple(p, d))
                .collect();
            return (AnnotationType::Union(members), None, None);
        }
        if let Some((positions, desc)) = cases.into_iter().next() {
            let trailing = first_trailing.unwrap_or("").trim();
            let is_tuple = positions.len() > 1
                || (!positions.is_empty() && (force_tuple || trailing.is_empty()));
            if is_tuple {
                return (AnnotationType::Tuple(positions, desc), None, None);
            }
        }
    }
    let (body, description) = split_legacy_desc(s);
    let type_only = extract_type_prefix(body);
    let trailing = body[type_only.len()..].trim();
    // LuaLS-style vararg return: `@return string ...` → VarArgs(string)
    if trailing == "..." {
        return (AnnotationType::VarArgs(Box::new(parse_type(type_only))), None, description);
    }
    let name = extract_trailing_ident(trailing);
    (parse_type(type_only), name, description)
}

pub(super) fn strip_return_description(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut end = s.len();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth = depth.saturating_sub(1),
            b'@' if depth == 0 && i > 0 && bytes[i - 1] == b' ' => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    s[..end].trim_end()
}

pub(super) fn find_hash_comment(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return Some(i),
            _ => {}
        }
    }
    None
}

pub(super) fn extract_type_prefix(s: &str) -> &str {
    let mut depth = 0usize;
    let mut after_colon = false;
    let mut in_fun_ret = false;
    let mut after_comma = false;
    let mut after_pipe = false;
    let mut after_ampersand = false;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                if !in_double_quote { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = false; }
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                if !in_single_quote { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = false; }
            }
            _ if in_single_quote || in_double_quote => {}
            '<' | '(' | '{' => { depth += 1; after_colon = false; in_fun_ret = false; after_comma = false; after_pipe = false; after_ampersand = false; }
            '>' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                after_colon = false;
                after_comma = false;
                after_pipe = false;
                after_ampersand = false;
                if depth == 0 && c == ')' {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            '|' if depth == 0 => { after_colon = false; after_comma = false; after_pipe = true; after_ampersand = false; }
            '&' if depth == 0 => { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = true; }
            ',' if depth == 0 && in_fun_ret => { after_comma = true; after_pipe = false; after_ampersand = false; }
            ':' if depth == 0 => { after_colon = true; after_pipe = false; after_ampersand = false; }
            c if c.is_whitespace() && depth == 0 && !after_colon && !after_comma && !after_pipe && !after_ampersand => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                if j < bytes.len() && (bytes[j] == b'|' || bytes[j] == b'&') {
                } else {
                    return &s[..i];
                }
            }
            _ => { after_colon = false; after_comma = false; after_pipe = false; after_ampersand = false; }
        }
    }
    s
}

fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' if !in_single_quote => { in_double_quote = !in_double_quote; }
            '\'' if !in_double_quote => { in_single_quote = !in_single_quote; }
            _ if in_single_quote || in_double_quote => {}
            '[' | '<' | '(' | '{' => depth += 1,
            ']' | '>' | ')' | '}' => {
                depth -= 1;
                if depth == 0 { return Some(i); }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn split_at_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut in_fun_ret = false;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '"' if !in_single_quote => { in_double_quote = !in_double_quote; }
            '\'' if !in_double_quote => { in_single_quote = !in_single_quote; }
            _ if in_single_quote || in_double_quote => {}
            '<' | '(' | '{' => { depth += 1; }
            '>' | ')' | '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && c == ')' {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b' ' { j += 1; }
                    if j < bytes.len() && bytes[j] == b':' {
                        in_fun_ret = true;
                    }
                }
            }
            c if c == sep && depth == 0 && !in_fun_ret => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn split_legacy_desc(s: &str) -> (&str, Option<String>) {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut at_pos: Option<usize> = None;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth = depth.saturating_sub(1),
            b'@' if depth == 0 && i > 0 && bytes[i - 1] == b' ' => {
                at_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    match at_pos {
        Some(p) => {
            let body = s[..p].trim_end();
            let desc = s[p + 1..].trim();
            let desc = if desc.is_empty() { None } else { Some(desc.to_string()) };
            (body, desc)
        }
        None => (s, None),
    }
}

fn extract_trailing_ident(s: &str) -> Option<String> {
    let first = s.split_whitespace().next().unwrap_or("");
    if first.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && first.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        Some(first.to_string())
    } else {
        None
    }
}

fn split_params(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => { parts.push(&s[start..i]); start = i + 1; }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}


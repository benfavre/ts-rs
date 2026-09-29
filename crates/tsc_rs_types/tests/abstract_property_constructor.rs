use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> Vec<Diagnostic> {
    check_with_options(source, &CompilerOptions::default())
}

fn check_with_options(source: &str, options: &CompilerOptions) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("abstract_property_constructor.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(&file, &symbols, options)
        .diagnostics
}

fn check_through_project_donor(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("abstract_property_constructor.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut donor = TypeChecker::new();
    donor.inject_external_types(&[&file]);
    donor.take_diagnostics();
    let mut checker = donor.clone();
    checker.set_current_file_name(&file.file_name);
    checker
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn ts2715_slices<'a>(source: &'a str, diagnostics: &[Diagnostic]) -> Vec<&'a str> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2715)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2715 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect()
}

#[test]
fn reports_each_abstract_property_read_once_during_initialization() {
    let source = r#"
abstract class Base {
    abstract value: string;
    abstract callback: () => void;

    constructor() {
        this.value.toLowerCase();
        this.callback();
    }

    direct = this.value;
    nested = () => this.value;
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["value", "callback", "value"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn no_unchecked_tuple_access_accepts_exhaustive_literal_index_unions() {
    let source = r#"
const tuple = ["a", "b"] as const;
declare const exact: 0 | 1;
declare const dynamic: number;
tuple[exact].toUpperCase();
tuple[dynamic].toUpperCase();
"#;
    let mut options = CompilerOptions::default();
    options.no_unchecked_indexed_access = Some(true);
    let diagnostics = check_with_options(source, &options);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2532)
            .count(),
        1,
        "only the dynamic tuple index should include undefined: {diagnostics:?}"
    );
}

#[test]
fn reports_abstract_getter_reads_during_initialization() {
    let source = r#"
abstract class Base {
    abstract get x(): string;
    abstract set y(value: string);

    constructor() {
        this.x;
        const { x } = this;
        const { y } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["x", "x", "y"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_named_abstract_properties_in_this_destructuring() {
    let source = r#"
abstract class Base {
    abstract x: string;
    abstract y: string;
    abstract 1: string;
    concrete = "";

    constructor() {
        const key = "x" as const;
        let { x, y: renamed, concrete } = this;
        ({ x, y: renamed, "y": renamed, concrete } = this);
        let { ['x']: computedX, 1: numericOne } = this;
        ({ ['x']: computedX, 1: numericOne } = this);
        let { [`x`]: templateX, [key]: constKeyX } = this;
        ({ [`x`]: templateX, [key]: constKeyX } = this);
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "x", "y", "x", "y", "\"y\"", "['x']", "1", "['x']", "1", "[`x`]", "[key]", "[`x`]",
            "[key]",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn concrete_properties_and_non_initialization_contexts_are_exempt() {
    let source = r#"
abstract class Base {
    abstract x: string;
    concrete = "";

    method(other: Base) {
        let { x } = this;
        let { x: otherX } = other;
    }

    constructor(other: Base) {
        const self = this;
        const { concrete } = this;
        const { x: otherX } = other;
        const { x: parenthesized } = (this);
        const { x: asserted } = this as Base;
        const { x: nonNull } = this!;
        const { x: satisfied } = this satisfies Base;
        ({ x: parenthesized } = (this));
        ({ x: asserted } = this as Base);
        ({ x: nonNull } = this!);
        ({ x: satisfied } = this satisfies Base);
        ({ x: compound } += this);
        (({ x: parenthesizedLeft }) = this);
        const nested = () => {
            const { x } = this;
        };
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn canonicalizes_numeric_and_unique_symbol_destructuring_keys() {
    let source = r#"
declare const sym: unique symbol;
abstract class Base {
    abstract 1: string;
    abstract [sym]: string;

    constructor() {
        let { 0x1: hex, 0b1: binary, 1e0: exponent, [0o1]: octal, [sym]: symbol } = this;
        ({ 0x1: hex, 0b1: binary, 1e0: exponent, [0o1]: octal, [sym]: symbol } = this);
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["0x1", "0b1", "1e0", "[0o1]", "[sym]", "0x1", "0b1", "1e0", "[0o1]", "[sym]",],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .any(|diagnostic| diagnostic.message.contains("property '[sym]'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn infers_unique_symbol_identity_from_symbol_factory_calls() {
    let source = r#"
const direct = Symbol();
const registered = Symbol.for("registered");
const parenthesized = (Symbol());
const optionalCall = Symbol?.();
const optionalMember = Symbol?.for("optional-member");
const optionalInvoke = Symbol.for?.("optional-invoke");
class Keys {
    static readonly sym = Symbol();
}
abstract class Base {
    abstract [direct]: string;
    abstract [registered]: string;
    abstract [parenthesized]: string;
    abstract [optionalCall]: string;
    abstract [optionalMember]: string;
    abstract [optionalInvoke]: string;
    abstract [Keys.sym]: string;

    constructor() {
        const {
            [direct]: directValue,
            [registered]: registeredValue,
            [parenthesized]: parenthesizedValue,
            [optionalCall]: optionalCallValue,
            [optionalMember]: optionalMemberValue,
            [optionalInvoke]: optionalInvokeValue,
            [Keys.sym]: staticValue,
        } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "[direct]",
            "[registered]",
            "[parenthesized]",
            "[optionalCall]",
            "[optionalMember]",
            "[optionalInvoke]",
            "[Keys.sym]",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn does_not_propagate_inferred_unique_identity_through_unannotated_aliases() {
    let source = r#"
const original = Symbol();
const alias = original;
let mutable = original;
class Keys {
    static readonly alias = original;
}
namespace N {
    export const alias = original;
}
abstract class Base {
    abstract [original]: string;
    constructor() {
        const { [alias]: a, [mutable]: b, [Keys.alias]: c, [N.alias]: d } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn module_local_symbol_factory_does_not_create_unique_identity() {
    let source = r#"
export {};
function Symbol(): symbol {
    throw new Error();
}
const local = Symbol();
abstract class Base {
    abstract [local]: string;
    constructor() {
        const { [local]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn destructured_symbol_binding_hides_the_global_factory() {
    let source = r#"
export {};
const { nested: { Symbol } } = {
    nested: { Symbol: (): symbol => { throw new Error(); } },
};
const key = Symbol();
abstract class Base {
    abstract [key]: string;
    constructor() {
        const { [key]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn type_space_symbol_declarations_do_not_hide_the_global_factory() {
    for declaration in ["interface Symbol {}", "type Symbol = {}"] {
        let source = format!(
            r#"
export {{}};
{declaration}
const key = Symbol();
abstract class Base {{
    abstract [key]: string;
    constructor() {{
        const {{ [key]: value }} = this;
    }}
}}
"#
        );
        let diagnostics = check(&source);
        assert_eq!(
            ts2715_slices(&source, &diagnostics),
            ["[key]"],
            "{declaration}: {diagnostics:?}"
        );
    }
}

#[test]
fn const_assertions_preserve_referenced_but_not_fresh_symbol_identity() {
    let source = r#"
const key = Symbol();
const referenced = { key } as const;
const fresh = { key: Symbol() } as const;
const identity = (value: typeof key) => value;
const returned = identity(key);
abstract class Base {
    abstract [referenced.key]: string;
    constructor() {
        const {
            [referenced.key]: referencedValue,
            [fresh.key]: freshValue,
            [returned]: returnedValue,
        } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[referenced.key]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn namespace_value_symbol_shadow_disables_factory_identity_for_the_whole_body() {
    let source = r#"
namespace Keys {
    export const key = Symbol();
    export function Symbol(): symbol {
        throw new Error();
    }
}
abstract class Base {
    abstract [Keys.key]: string;
    constructor() {
        const { [Keys.key]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn project_donor_preserves_unique_symbol_property_identity() {
    let source = r#"
declare const sym: unique symbol;
abstract class Base {
    abstract [sym]: string;

    constructor() {
        const { [sym]: value } = this;
    }
}
"#;
    let diagnostics = check_through_project_donor(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[sym]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("property '[sym]'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn project_donor_preserves_namespace_unique_symbol_property_identity() {
    let source = r#"
declare namespace Keys {
    const sym: unique symbol;
}
abstract class Base {
    abstract [Keys.sym]: string;

    constructor() {
        const { [Keys.sym]: value } = this;
    }
}
"#;
    let diagnostics = check_through_project_donor(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.sym]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn project_donor_refreshes_classes_after_later_key_providers() {
    let source = r#"
abstract class Base {
    abstract [Keys.sym]: string;

    constructor() {
        const { [Keys.sym]: value } = this;
    }
}
declare namespace Keys {
    const sym: unique symbol;
}
"#;
    let diagnostics = check_through_project_donor(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.sym]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn canonical_numeric_overrides_suppress_base_abstract_reads() {
    let source = r#"
abstract class Base {
    abstract 1: string;
}
abstract class Derived extends Base {
    1.0 = "";

    constructor() {
        super();
        const { 1: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn canonicalizes_negative_computed_numeric_keys() {
    let source = r#"
abstract class Base {
    abstract [-1]: string;
    abstract [-0]: string;

    constructor() {
        let { [-1]: negative, [(-1)]: parenthesized, [-0]: zero, [-0x1]: hex, [-0b1]: binary, [-0o1]: octal } = this;
        ({ [-1]: negative, [(-1)]: parenthesized, [-0]: zero, [-0x1]: hex, [-0b1]: binary, [-0o1]: octal } = this);
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "[-1]", "[(-1)]", "[-0]", "[-0x1]", "[-0b1]", "[-0o1]", "[-1]", "[(-1)]", "[-0]",
            "[-0x1]", "[-0b1]", "[-0o1]",
        ],
        "diagnostics: {diagnostics:?}"
    );
    let messages = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2715)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages[0].contains("property '[-1]'"), "{messages:?}");
    assert!(messages[2].contains("property '0'"), "{messages:?}");
}

#[test]
fn signed_radix_concrete_overrides_match_decimal_abstract_keys() {
    let source = r#"
abstract class Base {
    abstract [-1]: string;
}
abstract class Derived extends Base {
    [-0x1] = "";

    constructor() {
        super();
        const { [-1]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn numeric_threshold_overrides_use_javascript_property_names() {
    let source = r#"
abstract class Base {
    abstract 1e21: string;
    abstract 1e-7: string;
}
abstract class Derived extends Base {
    "1e+21" = "";
    "1e-7" = "";

    constructor() {
        super();
        const { 1e21: large, 1e-7: small } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn overflowing_numeric_overrides_use_infinity_property_names() {
    let source = r#"
abstract class Base {
    abstract 1e999: string;
    abstract [-1e999]: string;
}
abstract class Derived extends Base {
    "Infinity" = "";
    "-Infinity" = "";

    constructor() {
        super();
        const { 1e999: positive, [-1e999]: negative } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn arbitrary_width_radix_keys_match_their_javascript_number_value() {
    let source = r#"
abstract class Base {
    abstract [0x100000000000000000000000000000000]: string;

    constructor() {
        const { [3.402823669209385e38]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[3.402823669209385e38]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ordinary_symbol_computed_keys_are_not_unique_members() {
    let source = r#"
declare const sym: symbol;
abstract class Base {
    abstract [sym]: string;

    constructor() {
        const { [sym]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn invalid_mutable_unique_symbol_keys_do_not_gain_identity() {
    let source = r#"
declare let sym: unique symbol;
abstract class Base {
    abstract [sym]: string;

    constructor() {
        const { [sym]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn unique_symbol_aliases_share_computed_property_identity() {
    let direct = r#"
declare const sym: unique symbol;
declare const alias: typeof sym;
abstract class Base {
    abstract [sym]: string;

    constructor() {
        const { [alias]: value } = this;
    }
}
"#;
    let diagnostics = check(direct);
    assert_eq!(
        ts2715_slices(direct, &diagnostics),
        ["[alias]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("property '[sym]'")),
        "diagnostics: {diagnostics:?}"
    );

    let override_source = r#"
declare const sym: unique symbol;
declare const alias: typeof sym;
abstract class Base {
    abstract [sym]: string;
}
abstract class Derived extends Base {
    [alias] = "";

    constructor() {
        super();
        const { [sym]: value } = this;
    }
}
"#;
    let diagnostics = check(override_source);
    assert_eq!(
        ts2715_slices(override_source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_typeof_aliases_share_computed_property_identity() {
    let source = r#"
declare namespace Keys {
    const sym: unique symbol;
}
declare const alias: typeof Keys.sym;
abstract class Base {
    abstract [Keys.sym]: string;

    constructor() {
        const { [alias]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[alias]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_typeof_class_static_aliases_share_computed_property_identity() {
    let source = r#"
declare namespace N {
    const sym: unique symbol;
}
declare class Keys {
    static readonly alias: typeof N.sym;
}
abstract class Base {
    abstract [N.sym]: string;

    constructor() {
        const { [Keys.alias]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.alias]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn static_getter_literal_return_is_a_computed_property_key() {
    let source = r#"
class Keys {
    static get key(): "y" {
        return "y";
    }
}
abstract class Base {
    x = "";
    abstract y: string;

    constructor() {
        const { [Keys.key]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.key]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn destructuring_defaults_preserve_literal_computed_keys() {
    let source = r#"
abstract class Base {
    abstract x: string;

    constructor() {
        const { key = "x" } = {} as const;
        const { [key]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[key]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn mutable_const_assertions_and_computed_binding_keys_preserve_literal_types() {
    let source = r#"
abstract class Base {
    abstract x: string;
    abstract z: string;

    constructor() {
        let explicit = "x" as const;
        let { widened = "z" } = {} as const;
        const { ["key"]: projected } = { key: "x" } as const;
        const { [explicit]: explicitValue } = this;
        const { [widened]: widenedValue } = this;
        const { [projected]: projectedValue } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[explicit]", "[projected]"],
        "explicit const assertions stay narrow, mutable defaults widen: {diagnostics:?}"
    );
}

#[test]
fn destructuring_rest_omits_object_keys_and_slices_tuples() {
    let source = r#"
abstract class Base {
    x = "";
    abstract y: string;

    constructor() {
        const { key, ...rest } = { key: "x", other: "y" } as const;
        const [head, ...tail] = ["x", "y"] as const;
        rest.key;
        const { [tail[0]]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[tail[0]]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2339 && diagnostic.message.contains("'key'")),
        "object rest must omit consumed properties: {diagnostics:?}"
    );
}

#[test]
fn qualified_unique_symbols_share_computed_property_identity() {
    let source = r#"
declare class Keys {
    static readonly sym: unique symbol;
}
class DerivedKeys extends Keys {}
abstract class Base {
    abstract [Keys.sym]: string;

    constructor() {
        const { [Keys.sym]: value } = this;
        const { [DerivedKeys.sym]: inherited } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.sym]", "[DerivedKeys.sym]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .all(|diagnostic| diagnostic.message.contains("property '[Keys.sym]'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn namespace_unique_symbols_share_computed_property_identity() {
    let source = r#"
declare namespace Keys {
    const alias: typeof Keys.sym;
}
declare namespace Keys {
    const sym: unique symbol;
}
abstract class Base {
    abstract [Keys.sym]: string;

    constructor() {
        const { [Keys.sym]: value } = this;
        const { [Keys.alias]: alias } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.sym]", "[Keys.alias]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .all(|diagnostic| diagnostic.message.contains("property '[Keys.sym]'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn type_erasing_wrappers_preserve_computed_property_keys() {
    let source = r#"
abstract class Base {
    abstract x: string;

    constructor() {
        const {
            [("x" as const)]: asserted,
            [("x" satisfies string)]: satisfied,
            [("x"!)]: nonNull,
            [(<"x">"x")]: angleAsserted,
        } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "[(\"x\" as const)]",
            "[(\"x\" satisfies string)]",
            "[(\"x\"!)]",
            "[(<\"x\">\"x\")]",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn widening_assertions_are_not_constant_computed_property_keys() {
    let source = r#"
class Keys {
    static readonly x = "x" as const;
}
abstract class Base {
    abstract x: string;

    constructor() {
        const { [("x" as string)]: value } = this;
        const { [((Keys as any).x)]: ownerValue } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn qualified_literal_values_are_computed_property_keys() {
    let source = r#"
class Keys {
    static readonly x = "x" as const;
    static readonly one = 1 as const;
}
abstract class Base {
    abstract x: string;
    abstract 1: string;

    constructor() {
        const { [Keys.x]: stringValue, [Keys.one]: numberValue } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.x]", "[Keys.one]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn const_object_literal_values_are_computed_property_keys() {
    let source = r#"
const Keys = { x: "x", one: 1 } as const;
abstract class Base {
    abstract x: string;
    abstract 1: string;

    constructor() {
        const {
            [Keys.x]: stringValue,
            [Keys.one]: numberValue,
            [Keys["x"]]: elementStringValue,
            [Keys["one"]]: elementNumberValue,
        } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Keys.x]", "[Keys.one]", "[Keys[\"x\"]]", "[Keys[\"one\"]]"],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn enum_member_literals_are_computed_property_keys() {
    let source = r#"
enum Keys {
    \u0078 = "\x78",
    one = 1,
    two,
    computed = 1 + 1,
}
enum RadixKeys {
    A = 0x1,
    B,
}
enum NegativeKeys {
    A = -2,
    B,
}
abstract class Base {
    abstract x: string;
    abstract 1: string;
    abstract 0: string;
    abstract 2: string;
    abstract [-1]: string;

    constructor() {
        const {
            [Keys.x]: stringValue,
            [Keys.one]: numberValue,
            [Keys.two]: heterogeneousAutoValue,
            [Keys.computed]: computedValue,
            [RadixKeys.B]: radixAutoValue,
            [NegativeKeys.B]: negativeAutoValue,
        } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "[Keys.x]",
            "[Keys.one]",
            "[Keys.two]",
            "[Keys.computed]",
            "[RadixKeys.B]",
            "[NegativeKeys.B]",
        ],
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_namespace_unique_symbols_are_computed_property_keys() {
    let source = r#"
declare namespace Outer {
    namespace Inner {
        const alias: typeof Outer.Inner.sym;
    }
}
declare namespace Outer {
    namespace Inner {
        const sym: unique symbol;
    }
}
abstract class Base {
    abstract [Outer.Inner.sym]: string;

    constructor() {
        const { [Outer.Inner.alias]: value } = this;
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["[Outer.Inner.alias]"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .all(|diagnostic| diagnostic.message.contains("property '[Outer.Inner.sym]'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn shadowed_unique_symbols_keep_distinct_property_identity() {
    let source = r#"
declare const sym: unique symbol;
declare const outer: typeof sym;
abstract class Base {
    [outer] = "";
}
{
    const sym: unique symbol = Symbol();
    abstract class Derived extends Base {
        abstract [sym]: string;

        constructor() {
            super();
            const { [outer]: value } = this;
        }
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        Vec::<&str>::new(),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn escaped_string_and_template_keys_use_cooked_property_names() {
    let source = r#"
abstract class Base {
    abstract x: string;

    constructor() {
        const key = "\x78" as const;
        const templateKey = `\x78` as const;
        const { "\x78": stringKey, ['\u0078']: computedString, [`\x78`]: template, [key]: constKey, [templateKey]: constTemplateKey } = this;
        ({ "\x78": stringKey, ['\u0078']: computedString, [`\x78`]: template, [key]: constKey, [templateKey]: constTemplateKey } = this);
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        [
            "\"\\x78\"",
            "['\\u0078']",
            "[`\\x78`]",
            "[key]",
            "[templateKey]",
            "\"\\x78\"",
            "['\\u0078']",
            "[`\\x78`]",
            "[key]",
            "[templateKey]",
        ],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .all(|diagnostic| diagnostic.message.contains("property 'x'")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn escaped_identifiers_use_cooked_property_names() {
    let source = r#"
abstract class Base {
    abstract x: string;
    abstract \u0079: string;

    constructor() {
        this.\u0078;
        this.y;
        const { \u0078: escaped } = this;
        ({ \u0079: escaped } = this);
        const { \u0078 } = this;
        ({ \u0079 } = this);
    }
}
"#;
    let diagnostics = check(source);
    assert_eq!(
        ts2715_slices(source, &diagnostics),
        ["\\u0078", "y", "\\u0078", "\\u0079", "\\u0078", "\\u0079"],
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2715)
            .all(|diagnostic| {
                diagnostic.message.contains("property 'x'")
                    || diagnostic.message.contains("property 'y'")
            }),
        "diagnostics: {diagnostics:?}"
    );
}

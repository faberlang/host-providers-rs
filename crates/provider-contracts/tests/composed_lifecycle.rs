use host_kernel::{parse_manifest, Kernel};
use std::collections::{BTreeMap, BTreeSet};

struct ProviderCase {
    name: &'static str,
    prefix: &'static str,
    manifest_json: &'static str,
    register: fn(&mut Kernel) -> host_kernel::HostResult<()>,
    public_routes: &'static [&'static str],
    excluded_routes: &'static [&'static str],
}

const ALEATOR_ROUTES: &[&str] = &[
    "aleator:fractum",
    "aleator:sortire",
    "aleator:octetos",
    "aleator:uuid",
    "aleator:semina",
];

const CONSOLUM_ROUTES: &[&str] = &[
    "consolum:hauri",
    "consolum:hauriet",
    "consolum:lege",
    "consolum:leget",
    "consolum:funde",
    "consolum:scribe",
    "consolum:scribet",
    "consolum:dic",
    "consolum:dicet",
    "consolum:mone",
    "consolum:monet",
    "consolum:vide",
    "consolum:videbit",
    "consolum:audit",
    "consolum:loquitur",
    "consolum:admonet",
];

const PROCESSUS_ROUTES: &[&str] = &[
    "processus:exsequi",
    "processus:exsequetur",
    "processus:dimitte",
    "processus:lege",
    "processus:scribe",
    "processus:sedes",
    "processus:muta",
    "processus:identitas",
    "processus:argumenta",
    "processus:captura",
];

const SOLUM_ROUTES: &[&str] = &[
    "solum:lege",
    "solum:hauri",
    "solum:hauriet",
    "solum:partem",
    "solum:inveni",
    "solum:carpe",
    "solum:carpiet",
    "solum:scribe",
    "solum:scribet",
    "solum:funde",
    "solum:appone",
    "solum:apponet",
    "solum:exstat",
    "solum:exstabit",
    "solum:directoriumne",
    "solum:regularene",
    "solum:legibilene",
    "solum:vinculumne",
    "solum:mensura",
    "solum:modum",
    "solum:modus",
    "solum:vincula",
    "solum:dele",
    "solum:delet",
    "solum:exscribe",
    "solum:exscribet",
    "solum:renomina",
    "solum:renominabit",
    "solum:tange",
    "solum:tanget",
    "solum:sequere",
    "solum:sequetur",
    "solum:crea",
    "solum:creabit",
    "solum:enumera",
    "solum:enumerabit",
    "solum:amputa",
    "solum:amputabit",
    "solum:domus",
    "solum:temporarium",
    "solum:iunge",
    "solum:parens",
    "solum:nomen",
    "solum:suffixum",
    "solum:absolve",
];

const TEMPUS_ROUTES: &[&str] = &[
    "tempus:nunc",
    "tempus:monotonicum",
    "tempus:activum",
    "tempus:dormiet",
];

fn provider_cases() -> [ProviderCase; 5] {
    [
        ProviderCase {
            name: "aleator",
            prefix: "aleator",
            manifest_json: aleator::manifest_json(),
            register: aleator::register,
            public_routes: ALEATOR_ROUTES,
            excluded_routes: &[],
        },
        ProviderCase {
            name: "consolum",
            prefix: "consolum",
            manifest_json: consolum::manifest_json(),
            register: consolum::register,
            public_routes: CONSOLUM_ROUTES,
            excluded_routes: &["consolum:fundet"],
        },
        ProviderCase {
            name: "processus",
            prefix: "processus",
            manifest_json: processus::manifest_json(),
            register: processus::register,
            public_routes: PROCESSUS_ROUTES,
            excluded_routes: &["processus:exi"],
        },
        ProviderCase {
            name: "solum",
            prefix: "solum",
            manifest_json: solum::manifest_json(),
            register: solum::register,
            public_routes: SOLUM_ROUTES,
            excluded_routes: &["solum:fundet", "solum:leget"],
        },
        ProviderCase {
            name: "tempus",
            prefix: "tempus",
            manifest_json: tempus::manifest_json(),
            register: tempus::register,
            public_routes: TEMPUS_ROUTES,
            excluded_routes: &["tempus:expectet"],
        },
    ]
}

#[test]
fn composed_kernel_registers_unique_provider_identities_and_routes() {
    let cases = provider_cases();
    let mut kernel = Kernel::new();
    for case in &cases {
        (case.register)(&mut kernel)
            .unwrap_or_else(|error| panic!("register {}: {error}", case.name));
    }

    let manifest = kernel.manifest();
    assert_eq!(manifest.providers.len(), cases.len());

    let expected_names = cases.iter().map(|case| case.name).collect::<BTreeSet<_>>();
    let expected_prefixes = cases
        .iter()
        .map(|case| case.prefix)
        .collect::<BTreeSet<_>>();
    let actual_names = manifest
        .providers
        .iter()
        .map(|provider| provider.provider.as_str())
        .collect::<BTreeSet<_>>();
    let actual_prefixes = manifest
        .providers
        .iter()
        .flat_map(|provider| provider.prefixes.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();

    assert_eq!(actual_names, expected_names);
    assert_eq!(actual_prefixes, expected_prefixes);

    let providers_by_name = manifest
        .providers
        .iter()
        .map(|provider| (provider.provider.as_str(), provider))
        .collect::<BTreeMap<_, _>>();
    let mut admitted_routes = BTreeSet::new();

    for case in &cases {
        let standalone = parse_manifest(case.manifest_json)
            .unwrap_or_else(|error| panic!("parse {} manifest: {error}", case.name));
        let composed = providers_by_name
            .get(case.name)
            .unwrap_or_else(|| panic!("composed manifest missing {}", case.name));

        assert_eq!(composed.prefixes, vec![case.prefix.to_owned()]);
        assert_eq!(composed.calls, standalone.calls);

        let manifest_routes = standalone
            .calls
            .iter()
            .map(|call| call.route.as_str())
            .collect::<BTreeSet<_>>();
        let expected_routes = case.public_routes.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            manifest_routes, expected_routes,
            "{} route bijection",
            case.name
        );

        for route in case.public_routes {
            assert!(admitted_routes.insert(*route), "duplicate route {route}");
            assert!(kernel.supports_route(route), "kernel should admit {route}");
        }
        for route in case.excluded_routes {
            assert!(
                !manifest_routes.contains(route),
                "{} should not manifest {route}",
                case.name
            );
            assert!(!kernel.supports_route(route), "kernel should deny {route}");
        }
    }

    assert_eq!(admitted_routes.len(), 80);
}

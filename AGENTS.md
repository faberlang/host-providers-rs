# Agent Instructions

- This is the public provider workspace. Package names are the route-family
  names (`solum`, `processus`, `consolum`, `aleator`, `tempus`).
- Providers depend on `host-kernel`; kernel/native must never depend on a
  concrete provider.
- Each provider manifest is canonical and must agree with its dispatch table in
  both directions.
- Keep tests targeted and run workspace fmt, tests, and clippy before commits.

# Adding a port

**Follow these steps to add a port.** The New ports rule in `AGENTS.md` says when a capability needs a port.

1. Define the trait in a new file under `application/ports/`, named after the role.
   - If any of the trait's methods can fail, define an error type beside the trait.
   - Re-export the trait and its error type from `ports/mod.rs`.
2. Implement the trait in `adapters/`, in a type named after the technology and the role.
3. Add a hand-written fake in `src/test_support/`.
   - If the use cases need the port, wire the fake into `ServiceFixture`.
   - Otherwise, wire the fake into `CliHarness` or `UiHarness`.
4. Inject the port as a trait object.
   - If the use cases need the port, add the port to a bundle in `ServicePorts` as an `Arc<dyn Trait>`.
   - Otherwise, hand the port from `CompositionRoot` to the type that uses it, as with `Lock`.
5. Build the adapter in `CompositionRoot` and pass it in.
   - If the host can lack the technology behind the port, pass a null adapter. A null adapter does nothing on such a host. `NullPullRequests` is the null adapter for a host without `gh`.
   - A port is never an `Option`.

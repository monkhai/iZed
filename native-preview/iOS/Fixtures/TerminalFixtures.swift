// Appended to the actual MachinePicker.swift at build time so its file-private
// controllers remain private in production. Only fixture setup lives here.
@MainActor
func terminalCatalogScreen(_ id: String) -> UIViewController {
  let ready = id != "signed-out"
  let empty = id == "empty" || !ready
  let payload = PickerPayload(
    roster: Roster(
      machines: empty
        ? []
        : [
          Machine(
            name: "Studio Mac", hostname: "studio.demo.ts.net", address: "100.64.0.10", os: "macOS",
            online: true),
          Machine(
            name: "Build server", hostname: "build.demo.ts.net", address: "100.64.0.12",
            os: "Linux", online: true),
          Machine(
            name: "MacBook Pro", hostname: "macbook.demo.ts.net", address: "100.64.0.13",
            os: "macOS", online: false),
        ], updated_at: UInt64(Date().timeIntervalSince1970)),
    saved: empty
      ? []
      : [
        SavedHost(name: "Studio Mac", address: "demo@100.64.0.10", command: "")
      ], public_key: "DEMO PUBLIC KEY — NOT A CREDENTIAL",
    message: id == "roster-error" ? "Could not refresh Tailscale machines. Try again." : "",
    refreshing: id == "refreshing", has_session: false, tailnet_ready: ready)
  let picker = MachinePicker(payload)
  picker.loadViewIfNeeded()
  if id == "search-results" || id == "search-empty" {
    picker.search.text = id == "search-results" ? "Build" : "missing-machine"
    picker.filter()
  }
  if id.hasPrefix("connection") {
    let machine = Machine(
      name: "Build server", hostname: "build.demo.ts.net", address: "100.64.0.12", os: "Linux",
      online: true)
    let editor = ConnectionEditor(machine: machine, publicKey: payload.public_key)
    let nav = UINavigationController(rootViewController: editor)
    nav.overrideUserInterfaceStyle = .dark
    editor.loadViewIfNeeded()
    if id == "connection-advanced" {
      editor.command.isHidden = false
      editor.command.text = "tmux new-session -A -s phone"
      editor.username.text = "demo"
    }
    if id == "connection-invalid" { editor.connectNow() }
    if id == "connection-key-copied" {
      // Run the actual UIAction; only the explicitly invalid fixture key is copied.
      func find(_ view: UIView) -> UIButton? {
        if let b = view as? UIButton, b.title(for: .normal) == "Copy this phone’s public SSH key" {
          return b
        }
        return view.subviews.compactMap(find).first
      }
      find(editor.view)?.sendActions(for: .touchUpInside)
    }
    return nav
  }
  let isolated: UIView?
  switch id {
  case "search-field": isolated = picker.tableView.tableHeaderView
  case "picker-footer": isolated = picker.tableView.tableFooterView
  case "saved-row":
    isolated = picker.tableView(picker.tableView, cellForRowAt: IndexPath(row: 0, section: 0))
  case "online-row":
    isolated = picker.tableView(picker.tableView, cellForRowAt: IndexPath(row: 0, section: 1))
  case "offline-row":
    isolated = picker.tableView(picker.tableView, cellForRowAt: IndexPath(row: 1, section: 1))
  case "section-header": isolated = picker.tableView(picker.tableView, viewForHeaderInSection: 1)
  default: isolated = nil
  }
  if let isolated {
    return CatalogExtractedView(view: isolated, owner: picker)
  }
  let nav = UINavigationController(rootViewController: picker)
  nav.overrideUserInterfaceStyle = .dark
  return nav
}

@MainActor
func terminalCatalogActivate(_ controller: UIViewController, id: String) {
  guard let nav = controller as? UINavigationController,
    let picker = nav.topViewController as? MachinePicker
  else { return }
  MachinePicker.current = picker
  if id == "sign-out", nav.presentedViewController == nil { picker.confirmSignOut() }
  if id == "trust" || id == "trust-changed", nav.presentedViewController == nil {
    let alert = TerminalMakeTrustAlert(
      "demo@100.64.0.10", "SHA256:DEMO-FINGERPRINT-NOT-A-REAL-HOST", id == "trust-changed"
    ) { _ in }
    if let alert { picker.present(alert, animated: false) }
  }
}

@MainActor
func terminalCatalogState(_ controller: UIViewController) -> [String: Any] {
  var labels: [String] = []
  var fields: [String: String] = [:]
  func visit(_ view: UIView) {
    if let l = view as? UILabel, let text = l.text { labels.append(text) }
    if let f = view as? UITextField {
      fields[f.accessibilityIdentifier ?? f.placeholder ?? "field"] = f.text ?? ""
    }
    view.subviews.forEach(visit)
  }
  visit(controller.view)
  var modal = controller.presentedViewController
  while let current = modal {
    visit(current.view)
    modal = current.presentedViewController
  }
  return ["labels": labels, "fields": fields]
}

@MainActor
final class CatalogExtractedView: UIViewController {
  private let content: UIView
  private let owner: UIViewController
  init(view: UIView, owner: UIViewController) {
    self.content = view
    self.owner = owner
    super.init(nibName: nil, bundle: nil)
  }
  required init?(coder: NSCoder) { fatalError() }
  override func loadView() {
    view = UIView()
    view.backgroundColor = ZedStyle.background
    content.removeFromSuperview()
    view.addSubview(content)
  }
  override func viewDidLayoutSubviews() {
    super.viewDidLayoutSubviews()
    content.frame = view.bounds
    content.setNeedsLayout()
    content.layoutIfNeeded()
  }
}

@_cdecl("catalog_refresh_fixture")
func catalogRefreshFixture() {
  guard let current = MachinePicker.current else { return }
  let payload = PickerPayload(
    roster: current.payload.roster, saved: current.payload.saved,
    public_key: current.payload.public_key,
    message: "Local fixture refreshed · no network requests", refreshing: false,
    has_session: current.payload.has_session, tailnet_ready: current.payload.tailnet_ready)
  current.update(payload)
}

@MainActor
final class EmbeddedTailscale {
  static let shared = EmbeddedTailscale()
  func signIn(deviceName: String) async { setReady(true) }
  func updateDeviceName(_ name: String) async { catalogRefreshFixture() }
  func signOut() async { setReady(false) }
  private func setReady(_ ready: Bool) {
    guard let picker = MachinePicker.current else { return }
    let p = picker.payload
    picker.update(
      PickerPayload(
        roster: ready ? p.roster : Roster(machines: [], updated_at: 0), saved: ready ? p.saved : [],
        public_key: p.public_key,
        message: ready ? "Local sign-in fixture" : "Connect to Tailscale to see your machines.",
        refreshing: false, has_session: false, tailnet_ready: ready))
  }
}

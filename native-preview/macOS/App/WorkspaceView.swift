import SwiftUI

struct WorkspaceView: View {
  @State private var selected: String? = "machines"
  @State private var query = ""
  @State private var widths: [String: Double] = [:]
  @State private var revisions: [String: Int] = [:]
  private let groups = ["UIKit screens", "GPUI screens", "UIKit components", "GPUI components"]
  private var entry: CatalogEntry {
    catalogEntries.first { $0.id == selected } ?? catalogEntries[0]
  }
  var body: some View {
    NavigationSplitView {
      VStack(alignment: .leading, spacing: 12) {
        Text("iZed Terminal").font(.title2.bold())
        Text("\(catalogEntries.count) native previews").foregroundStyle(.secondary)
        TextField("Find a screen or component", text: $query).textFieldStyle(.roundedBorder)
        List(selection: $selected) {
          ForEach(groups, id: \.self) { group in
            Section(group) {
              ForEach(
                catalogEntries.filter {
                  $0.group == group
                    && (query.isEmpty || $0.title.localizedCaseInsensitiveContains(query))
                }
              ) { item in
                Text(item.title).tag(item.id)
              }
            }
          }
        }.listStyle(.sidebar)
      }.padding(16).navigationSplitViewColumnWidth(min: 250, ideal: 280, max: 330)
    } detail: {
      VStack(alignment: .leading, spacing: 12) {
        HStack {
          VStack(alignment: .leading, spacing: 6) {
            Text(entry.title).font(.title2.bold())
            Text("\(entry.renderer) · production rendering · local fixture data").foregroundStyle(
              .secondary)
          }
          Spacer()
          Button("Reconnect") { revisions[entry.id, default: 0] += 1 }
        }
        Text(entry.source).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
          .textSelection(.enabled)
        HStack {
          Text("Width")
          Slider(
            value: Binding(get: { widths[entry.id] ?? 390 }, set: { widths[entry.id] = $0 }),
            in: 280...560, step: 10
          )
          .accessibilityLabel("Preview width")
          Text("\(Int(widths[entry.id] ?? 390)) pt").monospacedDigit().frame(width: 64)
          Button("iPhone") { widths[entry.id] = 390 }
          Button("Wide") { widths[entry.id] = 540 }
        }
        Divider()
        ComponentCanvas(
          componentID: entry.id, overview: false, revision: revisions[entry.id, default: 0],
          width: widths[entry.id] ?? 390
        )
        .id(entry.id)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.black.opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
        Text("Runs in iOS Simulator → shared IOSurface → Metal. Click and type in the preview.")
          .font(.caption).foregroundStyle(.secondary)
      }.padding(24)
    }
  }
}

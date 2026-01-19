import SwiftUI

struct ContentView: View {
    var body: some View {
        NavigationSplitView {
            Text("Sidebar")
                .navigationSplitViewColumnWidth(min: 200, ideal: 250)
        } content: {
            Text("Content")
                .navigationSplitViewColumnWidth(min: 400, ideal: 500)
        } detail: {
            Text("Detail")
                .navigationSplitViewColumnWidth(min: 300, ideal: 350)
        }
        .frame(minWidth: 1000, minHeight: 600)
    }
}

#Preview {
    ContentView()
}

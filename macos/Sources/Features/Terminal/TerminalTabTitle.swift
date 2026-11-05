import SwiftUI

struct TerminalTabTitle: View {
    let item: TerminalTabModel.Item
    var horizontal = false

    var body: some View {
        Text(item.title)
            .font(.system(size: horizontal ? 12 : 13, weight: item.isSelected ? .medium : .regular))
            .lineLimit(1)
            .truncationMode(.middle)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

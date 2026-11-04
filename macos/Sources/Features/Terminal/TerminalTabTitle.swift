import SwiftUI

struct TerminalTabTitle: View {
    let item: TerminalTabModel.Item
    var horizontal = false

    var body: some View {
        HStack(spacing: 8) {
            if let status = item.gitStatus {
                Group {
                    if status.indicator == .draft {
                        DraftPullRequestIcon()
                            .fill(style: FillStyle(eoFill: true))
                            .frame(width: 16, height: 16)
                    } else if let name = status.indicator.assetName {
                        Image(name)
                            .resizable()
                            .frame(width: 16, height: 16)
                    } else {
                        Image(systemName: "arrow.triangle.branch")
                            .font(.system(size: 13))
                    }
                }
                .foregroundStyle(iconColor(status.indicator))
                .frame(width: 16)
            }
            Text(item.title)
                .font(.system(size: horizontal ? 12 : 13, weight: item.isSelected ? .medium : .regular))
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func iconColor(_ indicator: GitTabStatus.Indicator) -> Color {
        switch indicator {
        case .standard: .secondary
        case .open: .green
        case .draft: .primary
        case .merged: .purple
        case .conflict: .red
        }
    }
}

private struct DraftPullRequestIcon: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        for center in [CGPoint(x: 4, y: 2), CGPoint(x: 4, y: 14), CGPoint(x: 12, y: 14)] {
            path.addEllipse(in: CGRect(x: center.x - 2, y: center.y - 2, width: 4, height: 4))
            path.addEllipse(in: CGRect(x: center.x - 0.75, y: center.y - 0.75, width: 1.5, height: 1.5))
        }
        path.addRect(CGRect(x: 3.25, y: 4, width: 1.5, height: 8))
        for y in [2.0, 6.0, 10.0] {
            path.addEllipse(in: CGRect(x: 11, y: y - 1, width: 2, height: 2))
        }
        return path.applying(CGAffineTransform(a: rect.width / 16, b: 0, c: 0, d: rect.height / 16,
                                              tx: rect.minX, ty: rect.minY))
    }
}

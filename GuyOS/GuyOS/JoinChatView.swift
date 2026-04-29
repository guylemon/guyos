import SwiftUI

struct JoinChatView: View {
    var onJoin: (String) -> Void

    @Environment(\.dismiss) private var dismiss

    @State private var ticket: String = ""
    @State private var isScannerPresented: Bool = false
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            VStack(spacing: 12) {
                TextField("Ticket", text: $ticket, axis: .vertical)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .textFieldStyle(.roundedBorder)
                    .padding(.horizontal)

                HStack(spacing: 12) {
                    Button("Scan QR") { isScannerPresented = true }
                        .buttonStyle(.bordered)

                    Button("Join") { join() }
                        .buttonStyle(.borderedProminent)
                        .disabled(trimmedTicket.isEmpty)
                }
            }
            .padding(.vertical)
            .navigationTitle("Join chat")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
            .sheet(isPresented: $isScannerPresented) {
                QRCodeScannerSheet { scanned in
                    ticket = scanned
                    isScannerPresented = false
                    join()
                } onError: { err in
                    isScannerPresented = false
                    errorMessage = err
                }
            }
            .alert("Error", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } })) {
                Button("OK") { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "")
            }
        }
    }

    private var trimmedTicket: String { ticket.trimmingCharacters(in: .whitespacesAndNewlines) }

    private func join() {
        let t = trimmedTicket
        guard !t.isEmpty else { return }
        dismiss()
        onJoin(t)
    }
}

#Preview {
    JoinChatView { _ in }
}


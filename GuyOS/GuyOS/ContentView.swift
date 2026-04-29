//
//  ContentView.swift
//  GuyOS
//
//  Created by Guy Lemon on 4/29/26.
//

import SwiftUI
import GuyOSClient

struct ContentView: View {
    private enum SessionState {
        case choose
        case chatting
    }

    private enum StartMode {
        case open
        case join(ticket: String)
    }

    @State private var sessionState: SessionState = .choose
    @State private var isJoinPresented: Bool = false

    @State private var chat = GuyOSClient.Chat(name: "Guy")
    @State private var ticket: String = ""
    @State private var messages: [ChatMessage] = []
    @State private var inputText: String = ""
    @State private var errorMessage: String?
    @State private var receiverTask: Task<Void, Never>?

    var body: some View {
        NavigationStack {
            Group {
                switch sessionState {
                case .choose:
                    chooseView
                case .chatting:
                    chatView
                }
            }
            .navigationTitle("GuyOS Chat")
            .sheet(isPresented: $isJoinPresented) {
                JoinChatView { joinTicket in
                    Task { await startChat(mode: .join(ticket: joinTicket)) }
                }
            }
            .alert("Error", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } })) {
            Button("OK") { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "")
            }
        }
    }

    private var chooseView: some View {
        VStack(spacing: 16) {
            Button("Start chat") {
                Task { await startChat(mode: .open) }
            }
            .buttonStyle(.borderedProminent)

            Button("Join chat") {
                isJoinPresented = true
            }
            .buttonStyle(.bordered)
        }
        .padding()
    }

    private var chatView: some View {
        VStack {
            if !ticket.isEmpty {
                Text("Ticket: \(ticket)")
                    .font(.caption)
                    .textSelection(.enabled)
            }

            List(messages, id: \.id) { msg in
                VStack(alignment: .leading) {
                    Text(msg.from).font(.caption).foregroundStyle(.secondary)
                    Text(msg.text)
                }
            }

            HStack {
                TextField("Message", text: $inputText)
                Button("Send") {
                    Task { await sendMessage() }
                }
                .disabled(inputText.isEmpty)
            }
            .padding()
        }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Leave") { leaveChat() }
            }
        }
    }

    // MARK: - Chat logic

    private func startChat(mode: StartMode) async {
        do {
            receiverTask?.cancel()
            receiverTask = nil

            messages.removeAll(keepingCapacity: true)
            inputText = ""

            switch mode {
            case .open:
                ticket = try await chat.open()
                print("Opened room with ticket: \(ticket)")
            case .join(let joinTicket):
                ticket = joinTicket
                try await chat.join(ticketStr: joinTicket)
                print("Joined room with ticket: \(joinTicket)")
            }

            sessionState = .chatting
            startReceiverLoop()
        } catch {
            errorMessage = "Failed to start chat: \(error)"
        }
    }

    private func sendMessage() async {
        guard !inputText.isEmpty else { return }
        do {
            try await chat.send(text: inputText)
            inputText = ""
        } catch {
            errorMessage = "Send failed: \(error)"
        }
    }

    private func startReceiverLoop() {
        receiverTask?.cancel()
        receiverTask = Task {
            while !Task.isCancelled, let message = await chat.nextMessage() {
                await MainActor.run {
                    messages.append(message)
                }
            }
        }
    }

    private func leaveChat() {
        receiverTask?.cancel()
        receiverTask = nil

        ticket = ""
        messages.removeAll(keepingCapacity: false)
        inputText = ""

        sessionState = .choose
        isJoinPresented = false
    }
}

#Preview {
    ContentView()
}

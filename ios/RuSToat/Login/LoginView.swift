import SwiftUI

/// Logging in: server, email and password, then a two-factor code if the
/// account needs one. The steps follow desktop's login screen.
struct LoginView: View {
    let core: Core
    /// Why the user is here, if not by choice (shown until they try).
    var notice: String?
    /// Called once logged in.
    let onLoggedIn: (Account) -> Void

    private enum Step: Equatable {
        case credentials
        case mfa(instance: String, ticket: String, allowsTotp: Bool, allowsRecovery: Bool)
    }

    @State private var server = "https://api.stoat.chat"
    @State private var email = ""
    @State private var password = ""
    @State private var code = ""
    @State private var useRecovery = false
    @State private var step = Step.credentials
    @State private var error: String?
    @State private var busy = false
    @FocusState private var focus: Field?

    private enum Field { case server, email, password, code }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [.indigo, .purple, .pink.opacity(0.8)],
                startPoint: .topLeading, endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            ScrollView {
                VStack(spacing: 24) {
                    header
                    card
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 48)
                .frame(maxWidth: 480)
                .frame(maxWidth: .infinity)
            }
            .scrollDismissesKeyboard(.interactively)
        }
        .animation(.default, value: step)
        .animation(.default, value: error)
    }

    private var header: some View {
        VStack(spacing: 8) {
            Image(systemName: "bubble.left.and.text.bubble.right.fill")
                .font(.system(size: 48))
                .foregroundStyle(.white)
            Text("RuSToat")
                .font(.largeTitle.bold())
                .foregroundStyle(.white)
            Text(subtitle)
                .font(.subheadline)
                .foregroundStyle(.white.opacity(0.85))
                .multilineTextAlignment(.center)
        }
    }

    private var subtitle: String {
        switch step {
        case .credentials: "Log in to your Stoat account"
        case .mfa: useRecovery ? "Enter one of your recovery codes" : "Enter the code from your authenticator app"
        }
    }

    private var card: some View {
        VStack(alignment: .leading, spacing: 16) {
            switch step {
            case .credentials: credentialFields
            case .mfa(_, _, let allowsTotp, let allowsRecovery):
                mfaFields(allowsTotp: allowsTotp, allowsRecovery: allowsRecovery)
            }

            if let notice, error == nil, !busy, step == .credentials {
                Label(notice, systemImage: "info.circle.fill")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            if let error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .transition(.opacity)
            }

            Button(action: submit) {
                Group {
                    if busy {
                        ProgressView()
                    } else {
                        Text(step == .credentials ? "Log In" : "Continue")
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 4)
            }
            .buttonStyle(.glassProminent)
            .controlSize(.large)
            .disabled(busy)
        }
        .padding(24)
        .glassEffect(.regular, in: .rect(cornerRadius: 28))
    }

    @ViewBuilder private var credentialFields: some View {
        field("Server") {
            TextField("Server", text: $server, prompt: Text(verbatim: "api.stoat.chat"))
                .keyboardType(.URL)
                .textContentType(.URL)
                .focused($focus, equals: .server)
                .submitLabel(.next)
                .onSubmit { focus = .email }
        }
        field("Email") {
            TextField("Email", text: $email, prompt: Text(verbatim: "you@example.com"))
                .keyboardType(.emailAddress)
                .textContentType(.username)
                .focused($focus, equals: .email)
                .submitLabel(.next)
                .onSubmit { focus = .password }
        }
        field("Password") {
            SecureField("Password", text: $password)
                .textContentType(.password)
                .focused($focus, equals: .password)
                .submitLabel(.go)
                .onSubmit(submit)
        }
    }

    @ViewBuilder
    private func mfaFields(allowsTotp: Bool, allowsRecovery: Bool) -> some View {
        field(useRecovery ? "Recovery code" : "Authenticator code") {
            TextField(useRecovery ? "xxxx-xxxx" : "123456", text: $code)
                .keyboardType(useRecovery ? .asciiCapable : .numberPad)
                .textContentType(useRecovery ? nil : .oneTimeCode)
                .focused($focus, equals: .code)
                .submitLabel(.go)
                .onSubmit(submit)
        }
        HStack {
            Button("Back", systemImage: "chevron.left") {
                step = .credentials
                error = nil
                focus = .password
            }
            Spacer()
            if allowsTotp && allowsRecovery {
                Button(useRecovery ? "Use authenticator code" : "Use a recovery code") {
                    useRecovery.toggle()
                    code = ""
                    error = nil
                    focus = .code
                }
            }
        }
        .font(.footnote)
        .buttonStyle(.glass)
    }

    private func field(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            content()
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(.background.opacity(0.6), in: .rect(cornerRadius: 14))
        }
    }

    private func submit() {
        guard !busy else { return }
        busy = true
        error = nil
        let step = step
        Task {
            defer { busy = false }
            do {
                let outcome: LoginOutcome
                switch step {
                case .credentials:
                    outcome = try await core.login(server: server, email: email, password: password)
                case .mfa(let instance, let ticket, _, _):
                    let answer: MfaAnswer = useRecovery ? .recovery(code: code) : .totp(code: code)
                    outcome = try await core.loginMfa(instance: instance, ticket: ticket, answer: answer)
                }
                handle(outcome)
            } catch let CoreError.Failed(message) {
                error = message
            } catch {
                self.error = "Something went wrong. Please try again."
            }
        }
    }

    private func handle(_ outcome: LoginOutcome) {
        switch outcome {
        case .loggedIn(let account):
            onLoggedIn(account)
        case .needsMfa(let instance, let ticket, let allowsTotp, let allowsRecovery):
            // Show the address that worked.
            server = instance
            useRecovery = !allowsTotp
            code = ""
            step = .mfa(instance: instance, ticket: ticket, allowsTotp: allowsTotp, allowsRecovery: allowsRecovery)
            focus = .code
        case .disabled:
            error = "This account has been disabled."
        }
    }
}

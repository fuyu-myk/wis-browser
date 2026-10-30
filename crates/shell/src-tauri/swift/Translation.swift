// Bridge to Apple's Translation framework, which has no Objective-C API.
// Every call answers exactly once through its callback, from a Swift
// executor thread or, for the download sheet, the main thread. The framework
// does its work on the main run loop, so a caller must never block the main
// thread waiting for an answer.
import AppKit
import Foundation
import NaturalLanguage
import SwiftUI
import Translation

public typealias WisTranslationReply = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>) -> Void

/// The caller's callback and context. A C function pointer and an address
/// are immutable values, safe to hand to another thread.
struct Callback: @unchecked Sendable {
    let reply: WisTranslationReply
    let context: UnsafeMutableRawPointer?

    func answer(_ text: String) {
        text.withCString { reply(context, $0) }
    }
}

struct Run: Codable {
    let text: String
    let tag: Int?
}

struct TranslateRequest: Decodable {
    let source: String
    let target: String
    let strategy: String
    let runs: [Run]
    let skip: [Int]?
}

struct Reply: Encodable {
    var runs: [Run]? = nil
    var error: String? = nil
}

struct CheckRequest: Decodable {
    let source: String
}

struct CheckReply: Encodable {
    let target: String
    let sourceName: String
    let targetName: String
    let sameLanguage: Bool
    let readsSource: Bool
    let highFidelity: String
    let lowLatency: String
}

/// Translates one block: `request` is a `TranslateRequest` as JSON, and the
/// reply is a `Reply` as JSON, handed to `reply` along with `context`.
@_cdecl("wis_translation_translate")
public func wisTranslationTranslate(
    _ request: UnsafePointer<CChar>,
    _ context: UnsafeMutableRawPointer?,
    _ reply: WisTranslationReply
) {
    let body = Data(String(cString: request).utf8)
    let callback = Callback(reply: reply, context: context)
    Task.detached {
        callback.answer(await translate(body))
    }
}

struct DetectRequest: Decodable {
    let sample: String
}

struct DetectReply: Encodable {
    let language: String
    let confidence: Double
}

/// The language a text sample is in: `request` is a `DetectRequest` as JSON,
/// and the reply is a `DetectReply`, with an empty language when it can't tell.
@_cdecl("wis_translation_detect")
public func wisTranslationDetect(
    _ request: UnsafePointer<CChar>,
    _ context: UnsafeMutableRawPointer?,
    _ reply: WisTranslationReply
) {
    let body = Data(String(cString: request).utf8)
    let callback = Callback(reply: reply, context: context)
    Task.detached {
        callback.answer(detect(body))
    }
}

struct PrepareRequest: Decodable {
    let source: String
    let target: String
}

struct PrepareReply: Encodable {
    let status: String
}

func prepared(_ status: String) -> String {
    encode(PrepareReply(status: status))
}

/// Offers Apple's sheet that downloads a language pair for high fidelity, on
/// the window of `view` (an NSView). Main thread only: `request` is a
/// `PrepareRequest` as JSON, and the reply is a `PrepareReply` with the
/// pair's status once the sheet is done, or a `Reply` carrying an error.
@_cdecl("wis_translation_prepare")
public func wisTranslationPrepare(
    _ request: UnsafePointer<CChar>,
    _ view: UnsafeMutableRawPointer,
    _ context: UnsafeMutableRawPointer?,
    _ reply: WisTranslationReply
) {
    let body = Data(String(cString: request).utf8)
    let callback = Callback(reply: reply, context: context)
    let view = Unmanaged<NSView>.fromOpaque(view)
    MainActor.assumeIsolated {
        prepare(body, on: view.takeUnretainedValue(), callback)
    }
}

/// What translating from a language involves: `request` is a `CheckRequest`
/// as JSON, and the reply is a `CheckReply` or a `Reply` carrying an error.
@_cdecl("wis_translation_check")
public func wisTranslationCheck(
    _ request: UnsafePointer<CChar>,
    _ context: UnsafeMutableRawPointer?,
    _ reply: WisTranslationReply
) {
    let body = Data(String(cString: request).utf8)
    let callback = Callback(reply: reply, context: context)
    Task.detached {
        callback.answer(await check(body))
    }
}

func encode(_ reply: some Encodable) -> String {
    let data = (try? JSONEncoder().encode(reply)) ?? Data(#"{"error":"failed"}"#.utf8)
    return String(decoding: data, as: UTF8.self)
}

func translate(_ body: Data) async -> String {
    guard let request = try? JSONDecoder().decode(TranslateRequest.self, from: body) else {
        return encode(Reply(error: "badRequest"))
    }
    if request.runs.allSatisfy({ $0.text.allSatisfy(\.isWhitespace) }) {
        return encode(Reply(error: "nothingToTranslate"))
    }
    guard #available(macOS 26.4, *) else { return encode(Reply(error: "unavailable")) }
    guard let strategy = strategy(named: request.strategy) else {
        return encode(Reply(error: "badRequest"))
    }
    do {
        return encode(Reply(runs: try await translateBlock(request, strategy: strategy)))
    } catch {
        return encode(Reply(error: code(for: error)))
    }
}

func check(_ body: Data) async -> String {
    guard let request = try? JSONDecoder().decode(CheckRequest.self, from: body) else {
        return encode(Reply(error: "badRequest"))
    }
    if request.source.allSatisfy(\.isWhitespace) {
        return encode(Reply(error: "noLanguage"))
    }
    guard #available(macOS 26.4, *) else { return encode(Reply(error: "unavailable")) }
    guard let preferred = Locale.preferredLanguages.first else {
        return encode(Reply(error: "noTarget"))
    }
    let source = Locale.Language(identifier: request.source)
    let target = Locale.Language(identifier: preferred)
    return encode(CheckReply(
        target: target.minimalIdentifier,
        sourceName: displayName(of: source),
        targetName: displayName(of: target),
        sameLanguage: sameLanguage(source, target),
        readsSource: Locale.preferredLanguages.contains {
            sameLanguage(source, Locale.Language(identifier: $0))
        },
        highFidelity: await status(from: source, to: target, strategy: .highFidelity),
        lowLatency: await status(from: source, to: target, strategy: .lowLatency)
    ))
}

func detect(_ body: Data) -> String {
    guard let request = try? JSONDecoder().decode(DetectRequest.self, from: body) else {
        return encode(Reply(error: "badRequest"))
    }
    let recognizer = NLLanguageRecognizer()
    recognizer.processString(request.sample)
    let best = recognizer.languageHypotheses(withMaximum: 1).max { $0.value < $1.value }
    return encode(DetectReply(language: best?.key.rawValue ?? "", confidence: best?.value ?? 0))
}

@MainActor
func prepare(_ body: Data, on view: NSView, _ callback: Callback) {
    guard let request = try? JSONDecoder().decode(PrepareRequest.self, from: body) else {
        return callback.answer(encode(Reply(error: "badRequest")))
    }
    guard #available(macOS 26.4, *) else { return callback.answer(encode(Reply(error: "unavailable"))) }
    guard let content = view.window?.contentView else {
        return callback.answer(encode(Reply(error: "noWindow")))
    }
    content.addSubview(DownloadHost(
        source: Locale.Language(identifier: request.source),
        target: Locale.Language(identifier: request.target),
        callback: callback
    ))
}

/// Holds one download sheet and answers for it once: when the sheet is done,
/// or when the host leaves its window or the window closes. Wis keeps closed
/// windows, so closing frees nothing.
@available(macOS 26.4, *)
final class DownloadHost: NSView {
    private var callback: Callback?
    private var closing: (any NSObjectProtocol)?

    init(source: Locale.Language, target: Locale.Language, callback: Callback) {
        self.callback = callback
        super.init(frame: .zero)
        let sheet = NSHostingView(rootView: DownloadSheet(source: source, target: target, host: self))
        sheet.frame = .zero
        addSubview(sheet)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    private func answer(_ status: String) {
        guard let callback else { return }
        self.callback = nil
        callback.answer(prepared(status))
    }

    func finish(status: String) {
        answer(status)
        removeFromSuperview()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        closing.map(NotificationCenter.default.removeObserver)
        closing = nil
        guard let window else { return answer("supported") }
        closing = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.finish(status: "supported") }
        }
    }

    deinit {
        callback?.answer(prepared("supported"))
    }
}

@available(macOS 26.4, *)
struct DownloadSheet: View {
    let source: Locale.Language
    let target: Locale.Language
    weak var host: DownloadHost?

    var body: some View {
        Color.clear.translationTask(
            source: source,
            target: target,
            preferredStrategy: .highFidelity
        ) { @Sendable [weak host, source, target] session in
            try? await session.prepareTranslation()
            let now = await status(from: source, to: target, strategy: .highFidelity)
            await host?.finish(status: now)
        }
    }
}

/// The language and script decide it, so en-GB is English to an en-US reader
/// while zh-Hant is not zh-Hans.
func sameLanguage(_ a: Locale.Language, _ b: Locale.Language) -> Bool {
    let a = Locale.Language(identifier: a.maximalIdentifier)
    let b = Locale.Language(identifier: b.maximalIdentifier)
    return a.languageCode == b.languageCode && a.script == b.script
}

/// The name Wis shows, in English like the rest of its interface.
func displayName(of language: Locale.Language) -> String {
    Locale(identifier: "en").localizedString(forIdentifier: language.minimalIdentifier)
        ?? language.minimalIdentifier
}

@available(macOS 26.4, *)
func status(
    from source: Locale.Language,
    to target: Locale.Language,
    strategy: TranslationSession.Strategy
) async -> String {
    switch await LanguageAvailability(preferredStrategy: strategy).status(from: source, to: target) {
    case .installed: return "installed"
    case .supported: return "supported"
    case .unsupported: return "unsupported"
    @unknown default: return "unsupported"
    }
}

@available(macOS 26.4, *)
func strategy(named name: String) -> TranslationSession.Strategy? {
    switch name {
    case "highFidelity": return .highFidelity
    case "lowLatency": return .lowLatency
    default: return nil
    }
}

@available(macOS 26.4, *)
func code(for error: Error) -> String {
    let known: [(TranslationError, String)] = [
        (.notInstalled, "notInstalled"),
        (.unsupportedSourceLanguage, "unsupportedSourceLanguage"),
        (.unsupportedTargetLanguage, "unsupportedTargetLanguage"),
        (.unsupportedLanguagePairing, "unsupportedLanguagePairing"),
        (.unableToIdentifyLanguage, "unableToIdentifyLanguage"),
        (.nothingToTranslate, "nothingToTranslate"),
        (.alreadyCancelled, "cancelled"),
    ]
    return known.first { $0.0 ~= error }?.1 ?? "failed"
}

/// Which inline element of the block a run of text belongs to.
@available(macOS 26.4, *)
struct WisTag: AttributedStringKey {
    typealias Value = Int
    static let name = "wisTag"
}

/// A fresh session per block: creating one costs nothing measurable once the
/// model is loaded, and nothing is shared between concurrent requests.
@available(macOS 26.4, *)
func translateBlock(_ request: TranslateRequest, strategy: TranslationSession.Strategy) async throws -> [Run] {
    let session = TranslationSession(
        installedSource: Locale.Language(identifier: request.source),
        target: Locale.Language(identifier: request.target),
        preferredStrategy: strategy
    )
    let response = try await session.translate(attributed(request.runs, skip: Set(request.skip ?? [])))
    guard let out = response.attributedTargetText else {
        return [Run(text: response.targetText, tag: nil)]
    }
    return runs(of: out)
}

@available(macOS 26.4, *)
func attributed(_ runs: [Run], skip: Set<Int>) -> AttributedString {
    var text = AttributedString()
    for run in runs {
        var part = AttributedString(run.text)
        if let tag = run.tag {
            part[WisTag.self] = tag
            if skip.contains(tag) { part.translation.skipsTranslation = true }
        }
        text += part
    }
    return text
}

/// Adjacent pieces of the same inline element come back as one run.
@available(macOS 26.4, *)
func runs(of text: AttributedString) -> [Run] {
    var runs: [Run] = []
    for run in text.runs {
        let piece = String(text[run.range].characters)
        let tag = run[WisTag.self]
        if let last = runs.last, last.tag == tag {
            runs[runs.count - 1] = Run(text: last.text + piece, tag: tag)
        } else {
            runs.append(Run(text: piece, tag: tag))
        }
    }
    return runs
}

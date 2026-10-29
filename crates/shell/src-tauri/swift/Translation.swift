// Bridge to Apple's Translation framework, which has no Objective-C API.
// Every call answers exactly once through its callback, from a Swift
// executor thread. The framework does its work on the main run loop, so a
// caller must never block the main thread waiting for an answer.
import Foundation
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
}

struct Reply: Encodable {
    var runs: [Run]? = nil
    var error: String? = nil
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

func encode(_ reply: Reply) -> String {
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
    let response = try await session.translate(attributed(request.runs))
    guard let out = response.attributedTargetText else {
        return [Run(text: response.targetText, tag: nil)]
    }
    return runs(of: out)
}

@available(macOS 26.4, *)
func attributed(_ runs: [Run]) -> AttributedString {
    var text = AttributedString()
    for run in runs {
        var part = AttributedString(run.text)
        if let tag = run.tag { part[WisTag.self] = tag }
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

// StoreKit 2 bridge for ncrs-store (macOS 13 and later). Every function blocks until StoreKit has
// answered, so Rust must call them from a blocking thread, never from the main
// thread. Results are JSON in a C string that Rust gives back to
// ncrs_store_free.
import Foundation
import StoreKit

private let trialProductId = "dev.cosmicdrift.ncrs.trial14"

private final class ChangeCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var value: UInt64 = 0
    private var listener: Task<Void, Never>?

    func increment() {
        lock.lock()
        value += 1
        lock.unlock()
    }

    func read() -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return value
    }

    func setListener(_ task: Task<Void, Never>) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if listener != nil { return false }
        listener = task
        return true
    }
}

private let counter = ChangeCounter()

private final class ResultBox<T>: @unchecked Sendable {
    var value: T?
}

private func runBlocking<T>(_ body: @escaping @Sendable () async -> T) -> T? {
    let box = ResultBox<T>()
    let done = DispatchSemaphore(value: 0)
    Task.detached {
        box.value = await body()
        done.signal()
    }
    done.wait()
    return box.value
}

private func cString(_ object: [String: Any]) -> UnsafeMutablePointer<CChar>? {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
          let text = String(data: data, encoding: .utf8) else {
        return strdup("{\"error\":\"could not encode the answer\"}")
    }
    return strdup(text)
}

private func failure(_ key: String, _ message: String) -> UnsafeMutablePointer<CChar>? {
    cString([key: message])
}

private func readStatus() async -> [String: Any] {
    var status: [String: Any] = [
        "environment": NSNull(),
        "original_app_version": NSNull(),
        "trial_started_at": NSNull(),
        "full_purchased": false,
        "unsupported": false,
        "error": NSNull(),
    ]
    do {
        switch try await AppTransaction.shared {
        case .verified(let app):
            switch app.environment {
            case .production: status["environment"] = "production"
            case .sandbox: status["environment"] = "sandbox"
            case .xcode: status["environment"] = "xcode"
            default: status["environment"] = NSNull()
            }
            status["original_app_version"] = app.originalAppVersion
        case .unverified(_, let reason):
            status["error"] = "app transaction not verified: \(reason)"
            return status
        }
    } catch {
        status["error"] = "app transaction: \(error)"
        return status
    }
    var trialStart: Date?
    var full = false
    for await entitlement in Transaction.currentEntitlements {
        guard case .verified(let transaction) = entitlement, transaction.revocationDate == nil else {
            continue
        }
        if transaction.productID == trialProductId {
            if trialStart.map({ transaction.originalPurchaseDate < $0 }) ?? true {
                trialStart = transaction.originalPurchaseDate
            }
        } else if transaction.productID == "dev.cosmicdrift.ncrs.fullversion" {
            full = true
        }
    }
    status["trial_started_at"] = trialStart.map { Int($0.timeIntervalSince1970) } ?? NSNull()
    status["full_purchased"] = full
    return status
}

private func statusJson() -> UnsafeMutablePointer<CChar>? {
    guard let status = runBlocking({ await readStatus() }) else {
        return failure("error", "no answer from StoreKit")
    }
    return cString(status)
}

@_cdecl("ncrs_store_start_listener")
public func ncrs_store_start_listener() {
    let task = Task.detached {
        for await update in Transaction.updates {
            if case .verified(let transaction) = update {
                await transaction.finish()
                counter.increment()
            }
        }
    }
    if !counter.setListener(task) { task.cancel() }
}

@_cdecl("ncrs_store_changes")
public func ncrs_store_changes() -> UInt64 {
    counter.read()
}

@_cdecl("ncrs_store_status")
public func ncrs_store_status() -> UnsafeMutablePointer<CChar>? {
    statusJson()
}

private func buy(_ productId: String) async -> [String: Any] {
    do {
        guard let product = try await Product.products(for: [productId]).first else {
            return ["outcome": "failed", "error": "product not found: \(productId)"]
        }
        switch try await product.purchase() {
        case .success(let verification):
            guard case .verified(let transaction) = verification else {
                return ["outcome": "failed", "error": "transaction not verified"]
            }
            await transaction.finish()
            counter.increment()
            return ["outcome": "purchased"]
        case .userCancelled:
            return ["outcome": "cancelled"]
        case .pending:
            return ["outcome": "pending"]
        @unknown default:
            return ["outcome": "failed", "error": "unknown purchase result"]
        }
    } catch {
        return ["outcome": "failed", "error": "\(error)"]
    }
}

@_cdecl("ncrs_store_purchase")
public func ncrs_store_purchase(_ productId: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    guard let productId else {
        return cString(["outcome": "failed", "error": "no product id"])
    }
    let id = String(cString: productId)
    guard let result = runBlocking({ await buy(id) }) else {
        return cString(["outcome": "failed", "error": "no answer from StoreKit"])
    }
    return cString(result)
}

private func sync() async -> String? {
    do {
        try await AppStore.sync()
        return nil
    } catch {
        return "\(error)"
    }
}

@_cdecl("ncrs_store_restore")
public func ncrs_store_restore() -> UnsafeMutablePointer<CChar>? {
    if let message = runBlocking({ await sync() }) ?? nil {
        return failure("error", "restore: \(message)")
    }
    counter.increment()
    return statusJson()
}

@_cdecl("ncrs_store_free")
public func ncrs_store_free(_ pointer: UnsafeMutablePointer<CChar>?) {
    free(pointer)
}

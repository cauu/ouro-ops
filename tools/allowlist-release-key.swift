#!/usr/bin/env swift

import CryptoKit
import Darwin
import Foundation
import Security

// S0019 p10-5 — macOS-only release authority for data/allowlist.json.
//
// The raw Ed25519 seed is stored as a non-synchronizing, user-presence-protected generic-password
// item in the macOS login Keychain with an empty trusted-application ACL, so every reading
// application requires explicit Keychain authorization. It is never accepted through argv/stdin,
// written to disk, or printed. This helper is deliberately creation-only; the feature-gated Rust
// signer owns canonicalization, strict semantic validation and signing.

private let service = "io.ouro-ops.allowlist-release"
private let account = "production-ed25519-2026-07"
private let label = "Ouro Ops Allowlist Release Private Key (2026-07)"

private enum ToolError: Error, CustomStringConvertible {
    case usage
    case keychain(String)
    case keyAlreadyExists

    var description: String {
        switch self {
        case .usage:
            return "usage: allowlist-release-key.swift create"
        case .keychain(let message):
            return "Keychain error: \(message)"
        case .keyAlreadyExists:
            return "release key already exists; refusing to overwrite it"
        }
    }
}

private func statusMessage(_ status: OSStatus) -> String {
    if let message = SecCopyErrorMessageString(status, nil) as String? {
        return "\(message) (\(status))"
    }
    return "OSStatus \(status)"
}

private func keyQuery() -> [String: Any] {
    [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: service,
        kSecAttrAccount as String: account,
    ]
}

private func keyExists() throws -> Bool {
    var query = keyQuery()
    query[kSecReturnAttributes as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    switch status {
    case errSecSuccess:
        return true
    case errSecItemNotFound:
        return false
    default:
        throw ToolError.keychain(statusMessage(status))
    }
}

private func createKey() throws -> Curve25519.Signing.PrivateKey {
    if try keyExists() {
        throw ToolError.keyAlreadyExists
    }

    let privateKey = Curve25519.Signing.PrivateKey()
    var access: SecAccess?
    let accessStatus = SecAccessCreate(label as CFString, [] as CFArray, &access)
    guard accessStatus == errSecSuccess, let access else {
        throw ToolError.keychain(statusMessage(accessStatus))
    }

    var item = keyQuery()
    item[kSecAttrLabel as String] = label
    item[kSecAttrAccess as String] = access
    item[kSecValueData as String] = privateKey.rawRepresentation
    let addStatus = SecItemAdd(item as CFDictionary, nil)
    guard addStatus == errSecSuccess else {
        throw ToolError.keychain(statusMessage(addStatus))
    }
    return privateKey
}

private func hex(_ bytes: some Sequence<UInt8>) -> String {
    bytes.map { String(format: "%02x", $0) }.joined()
}

private func publicResult(_ privateKey: Curve25519.Signing.PrivateKey, created: Bool?) -> [String: Any] {
    var result: [String: Any] = [
        "account": account,
        "public_key": hex(privateKey.publicKey.rawRepresentation),
        "service": service,
    ]
    if let created {
        result["created"] = created
    }
    return result
}

private func printJSON(_ object: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    guard let text = String(data: data, encoding: .utf8) else {
        throw ToolError.keychain("result is not UTF-8")
    }
    print(text)
}

do {
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard let command = arguments.first else {
        throw ToolError.usage
    }
    switch command {
    case "create" where arguments.count == 1:
        try printJSON(publicResult(try createKey(), created: true))
    default:
        throw ToolError.usage
    }
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(1)
}

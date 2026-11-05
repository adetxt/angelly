import Foundation
import Darwin

/// Called from the Git actors so a subprocess never blocks the UI thread.
enum GitCommandRunner {
    struct Result {
        let exitCode: Int32
        let output: String
    }

    static func run(
        executable: URL,
        arguments: [String],
        directory: URL? = nil,
        timeout: TimeInterval? = nil,
        maxOutputBytes: Int? = nil
    ) throws -> Result {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.currentDirectoryURL = directory
        process.environment = environment(from: ProcessInfo.processInfo.environment)
        process.standardInput = FileHandle.nullDevice
        // File output prevents descendants holding stdout open from defeating the timeout.
        let manager = FileManager.default
        let outputDirectory = manager.temporaryDirectory.appendingPathComponent("angelly-git-\(UUID().uuidString)")
        try manager.createDirectory(at: outputDirectory, withIntermediateDirectories: false,
                                    attributes: [.posixPermissions: 0o700])
        defer { try? manager.removeItem(at: outputDirectory) }
        let outputURL = outputDirectory.appendingPathComponent("output")
        guard manager.createFile(atPath: outputURL.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        let file = try FileHandle(forWritingTo: outputURL)
        defer { try? file.close() }
        process.standardOutput = file
        process.standardError = file
        try process.run()

        let expiry = timeout.map { duration in
            let work = DispatchWorkItem {
                if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + duration, execute: work)
            return work
        }
        defer { expiry?.cancel() }

        process.waitUntilExit()
        let data: Data
        if let maxOutputBytes {
            guard maxOutputBytes >= 0, maxOutputBytes < Int.max else {
                throw GitWorktreeService.Failure(message: "Invalid command output limit.")
            }
            let reader = try FileHandle(forReadingFrom: outputURL)
            defer { try? reader.close() }
            data = try reader.read(upToCount: maxOutputBytes + 1) ?? Data()
            guard data.count <= maxOutputBytes else {
                throw GitWorktreeService.Failure(message: "The command returned too much output.")
            }
        } else {
            data = try Data(contentsOf: outputURL)
        }
        guard let output = String(data: data, encoding: .utf8) else {
            throw GitWorktreeService.Failure(message: "The command returned text that isn't valid UTF-8.")
        }
        return Result(exitCode: process.terminationStatus, output: output)
    }

    static func environment(from inherited: [String: String]) -> [String: String] {
        // Repository overrides must not redirect commands away from the tab's checkout.
        var environment = inherited.filter { !$0.key.hasPrefix("GIT_") && $0.key != "GH_REPO" && $0.key != "GH_HOST" }
        environment["GIT_TERMINAL_PROMPT"] = "0"
        environment["GIT_OPTIONAL_LOCKS"] = "0"
        environment["GH_PROMPT_DISABLED"] = "1"
        environment["GH_NO_UPDATE_NOTIFIER"] = "1"
        environment["NO_COLOR"] = "1"
        environment["LC_ALL"] = "C"
        return environment
    }
}

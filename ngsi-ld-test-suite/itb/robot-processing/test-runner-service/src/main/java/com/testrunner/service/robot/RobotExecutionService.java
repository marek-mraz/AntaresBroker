package com.testrunner.service.robot;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

import org.springframework.stereotype.Service;

@Service
public class RobotExecutionService {
    
    public void execute(RobotExecutionConfig config)
            throws IOException, InterruptedException {

        List<String> command = buildCommand(config);

        ProcessBuilder pb = new ProcessBuilder(command);
        pb.directory(config.getWorkingDirectory().toFile());
        pb.redirectErrorStream(true);

        Process process = pb.start();
        logProcessOutput(process);

        int exitCode = process.waitFor();

        if (exitCode > 1) {
            handleExitCode(exitCode, config);
        }
    }

    // ---------------------------------------------------------------------
    // Command construction
    // ---------------------------------------------------------------------

    private List<String> buildCommand(RobotExecutionConfig config) {
        List<String> cmd = new ArrayList<>();

        cmd.add("robot");

        if (config.isDryRun()) {
            cmd.add("--dryrun");
        }

        if (config.getListener() != null && !config.getListener().isBlank()) {
            cmd.add("--listener");
            cmd.add(config.getListener());
        }

        if (config.getTestName() != null) {
            cmd.add("--test");
            cmd.add(config.getTestName());
        }

        if (config.getSuiteName() != null) {
            cmd.add("--suite");
            cmd.add(config.getSuiteName());
        }

        if (config.getOutputDir() != null) {
            cmd.add("--outputdir");
            cmd.add(config.getOutputDir().toString());
        }

        if (config.getVariables() != null) {
            for (String variable : config.getVariables()) {
                cmd.add("--variable");
                cmd.add(variable);
            }
        }

        cmd.add(resolveTargetPath(config));

        return cmd;
    }

    private String resolveTargetPath(RobotExecutionConfig config) {
        Path target = config.getTargetPath() != null
                ? config.getTargetPath()
                : config.getWorkingDirectory();

        return target.toString();
    }

    // ---------------------------------------------------------------------
    // Exit code handling
    // ---------------------------------------------------------------------

    private void handleExitCode(int exitCode, RobotExecutionConfig config)
            throws IOException {

        if (exitCode == 0) {
            return;
        }

        if (config.isDryRun()) {
            System.out.println(
                "[WARN] Robot dry-run finished with exit code "
                + exitCode + " (ignored)"
            );
            return;
        }

        throw new IOException(
            "Robot execution failed with exit code " + exitCode
        );
    }

    // ---------------------------------------------------------------------
    // Logging
    // ---------------------------------------------------------------------

    private void logProcessOutput(Process process) throws IOException {
        try (BufferedReader reader =
                     new BufferedReader(new InputStreamReader(process.getInputStream()))) {

            String line;
            while ((line = reader.readLine()) != null) {
                System.out.println("[ROBOT] " + line);
            }
        }
    }
}

package com.testrunner.service;

import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Comparator;
import java.util.List;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
public class GitService {

    private final Path workingDir;

    public GitService(
        @Value("${testrunner.working-directory:/opt/robot-tests/working-repo}")
        String workingDirPath
    ) {
        this.workingDir = Paths.get(workingDirPath);
    }

    public Path getWorkingDir() {
        return workingDir;
    }

    public void cloneRepository(String repoUrl, Path targetDir)
            throws IOException, InterruptedException {

        // No needs to keep the previous cloned repo
        if (Files.exists(targetDir)) {
            deleteDirectoryRecursively(targetDir);
        }

        Files.createDirectories(targetDir.getParent());

        List<String> gitCmd = List.of(
                "git",
                "clone",
                repoUrl,
                targetDir.toString()
        );

        ProcessBuilder gitPb = new ProcessBuilder(gitCmd);
        gitPb.redirectErrorStream(true);

        Process gitProcess = gitPb.start();
        logProcessOutput(gitProcess);

        int gitExit = gitProcess.waitFor();
        if (gitExit != 0) {
            throw new IOException("Error cloning the Git repo: exit code " + gitExit);
        }
    }

    private void deleteDirectoryRecursively(Path path) throws IOException {
        if (!Files.exists(path)) return;

        Files.walk(path)
                .sorted(Comparator.reverseOrder())
                .map(Path::toFile)
                .forEach(File::delete);
    }

    private void logProcessOutput(Process process) throws IOException {
        try (BufferedReader reader =
                    new BufferedReader(new InputStreamReader(process.getInputStream()))) {
            reader.lines().forEach(System.out::println);
        }
    }
}
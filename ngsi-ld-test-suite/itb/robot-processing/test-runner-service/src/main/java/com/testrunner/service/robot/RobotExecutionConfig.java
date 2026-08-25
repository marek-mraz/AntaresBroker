package com.testrunner.service.robot;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

public class RobotExecutionConfig {

    private Path workingDirectory;

    private Path targetPath;

    // === OPTIONS ROBOT ===
    private boolean dryRun = false;

    private String listener;

    private List<String> includeTags = new ArrayList<>();

    private List<String> excludeTags = new ArrayList<>();
    
    private String testName;
    
    private String suiteName;
    
    private List<String> variables = new ArrayList<>();
    
    private String logLevel = "INFO";
    
    private Integer timeoutSeconds;
    
    private Path outputDir;

    // === GETTERS / SETTERS ===

    public Path getWorkingDirectory() {
        return workingDirectory;
    }

    public void setWorkingDirectory(Path workingDirectory) {
        this.workingDirectory = workingDirectory;
    }

    public Path getTargetPath() {
        return targetPath;
    }

    public void setTargetPath(Path targetPath) {
        this.targetPath = targetPath;
    }

    public boolean isDryRun() {
        return dryRun;
    }

    public void setDryRun(boolean dryRun) {
        this.dryRun = dryRun;
    }

    public String getListener() {
        return listener;
    }

    public void setListener(String listener) {
        this.listener = listener;
    }

    public List<String> getIncludeTags() {
        return includeTags;
    }

    public List<String> getExcludeTags() {
        return excludeTags;
    }

    public String getTestName() {
        return testName;
    }

    public void setTestName(String testName) {
        this.testName = testName;
    }

    public String getSuiteName() {
        return suiteName;
    }

    public void setSuiteName(String suiteName) {
        this.suiteName = suiteName;
    }

    public List<String> getVariables() {
        return variables;
    }

    public void addVariable(String key, String value) {
        this.variables.add(key + ":" + value);
    }

    public String getLogLevel() {
        return logLevel;
    }

    public void setLogLevel(String logLevel) {
        this.logLevel = logLevel;
    }

    public Integer getTimeoutSeconds() {
        return timeoutSeconds;
    }

    public void setTimeoutSeconds(Integer timeoutSeconds) {
        this.timeoutSeconds = timeoutSeconds;
    }

    public Path getOutputDir() {
        return outputDir;
    }

    public void setOutputDir(Path outputDir) {
        this.outputDir = outputDir;
    }
}

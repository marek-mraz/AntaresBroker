package com.testrunner.model.itb;

import java.util.*;

import com.testrunner.model.robot.KeywordStep;

public class Scenario {

    private String path;
    private String testName;
    private String description;
    private List<String> actors;

    private List<KeywordStep> setup;
    private List<KeywordStep> steps;
    private List<KeywordStep> teardown;

    public String getPath() {
        return path;
    }

    public void setPath(String path) {
        this.path = path;
    }

    public String getTestName() {
        return testName;
    }

    public void setTestName(String testName) {
        this.testName = testName;
    }

    public String getTestId() {
        return testName.replaceAll("\\s+", "_")
                       .replaceAll("[^a-zA-Z0-9_]", "");
    }

    public String getDescription() {
        return description;
    }

    public void setDescription(String description) {
        this.description = description;
    }

    public List<String> getActors() {
        return actors;
    }

    public void setActors(List<String> actors) {
        this.actors = actors;
    }

    public List<KeywordStep> getSetup() {
        return setup;
    }

    public void setSetup(List<KeywordStep> setup) {
        this.setup = setup;
    }

    public List<KeywordStep> getSteps() {
        return steps;
    }

    public void setSteps(List<KeywordStep> steps) {
        this.steps = steps;
    }

    public List<KeywordStep> getTeardown() {
        return teardown;
    }

    public void setTeardown(List<KeywordStep> teardown) {
        this.teardown = teardown;
    }
}

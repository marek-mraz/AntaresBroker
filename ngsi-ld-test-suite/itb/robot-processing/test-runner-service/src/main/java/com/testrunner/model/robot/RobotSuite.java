package com.testrunner.model.robot;

import java.util.*;

import com.fasterxml.jackson.annotation.JsonProperty;

public class RobotSuite {

    private String description;

    private List<String> actors;

    @JsonProperty("SETUP")
    private List<KeywordStep> setup;
    
    @JsonProperty("TEARDOWN")
    private List<KeywordStep> teardown;

    @JsonProperty("TESTS")
    private Map<String, List<KeywordStep>> tests;

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

    public List<KeywordStep> getTeardown() {
        return teardown;
    }

    public void setTeardown(List<KeywordStep> teardown) {
        this.teardown = teardown;
    }

    public Map<String, List<KeywordStep>> getTests() {
        return tests;
    }

    public void setTests(Map<String, List<KeywordStep>> tests) {
        this.tests = tests;
    }
}

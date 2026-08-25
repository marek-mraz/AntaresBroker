package com.testrunner.service;

import com.testrunner.model.itb.*;
import com.testrunner.model.robot.*;

import java.util.*;

public class ScenarioFactory {

    private ScenarioFactory() {
    }

    public static List<Scenario> fromRobotReport(RobotReport report) {
        List<Scenario> scenarios = new ArrayList<>();

        if (report == null || report.getReport() == null) {
            return scenarios;
        }

        for (Map.Entry<String, RobotSuite> suiteEntry : report.getReport().entrySet()) {

            String suitePath = suiteEntry.getKey();
            RobotSuite suite = suiteEntry.getValue();

            if (suite.getTests() == null) {
                continue;
            }

            for (Map.Entry<String, List<KeywordStep>> testEntry : suite.getTests().entrySet()) {

                Scenario scenario = new Scenario();
                scenario.setPath(suitePath);
                scenario.setTestName(testEntry.getKey());
                scenario.setDescription(suite.getDescription());
                scenario.setActors(suite.getActors());

                scenario.setSetup(cloneSteps(suite.getSetup()));
                scenario.setSteps(cloneSteps(testEntry.getValue()));
                scenario.setTeardown(cloneSteps(suite.getTeardown()));

                scenarios.add(scenario);
            }
        }
        return scenarios;
    }

    private static List<KeywordStep> cloneSteps(List<KeywordStep> source) {
        if (source == null) return List.of();

        return source.stream()
                .map(step -> {
                    KeywordStep ks = new KeywordStep();
                    ks.setKeyword(step.getKeyword());
                    ks.setActor(step.getActor());
                    return ks;
                })
                .toList();
    }
}
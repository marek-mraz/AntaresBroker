package com.testrunner.model.robot;

import com.fasterxml.jackson.annotation.JsonAnySetter;

import java.util.*;

public class RobotReport {
    private Map<String, RobotSuite> report = new HashMap<>();

    @JsonAnySetter
    public void addReport(String key, RobotSuite value) {
        report.put(key,value);
    }
    
    public Map<String, RobotSuite> getReport() {
        return report;
    }
}

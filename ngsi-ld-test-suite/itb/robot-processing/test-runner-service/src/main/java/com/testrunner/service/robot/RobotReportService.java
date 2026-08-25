package com.testrunner.service.robot;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.stereotype.Service;

import java.io.*;
import java.nio.file.*;

import com.testrunner.model.robot.*;

@Service
public class RobotReportService {

    private final ObjectMapper objectMapper = new ObjectMapper();

    public RobotReport loadReport(Path reportPath) throws IOException {
        return objectMapper.readValue(
                reportPath.toFile(),
                RobotReport.class
        );
    }
}
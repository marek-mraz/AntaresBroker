package com.testrunner.service;

import org.springframework.stereotype.Service;

import com.testrunner.model.itb.*;
import com.testrunner.model.robot.*;
import com.testrunner.service.robot.*;

import java.nio.file.*;
import java.util.*;

@Service
public class InitWorkflowService {

    private final GitService gitService;
    private final RobotExecutionService robotExecutionService;
    private final RobotReportService robotReportService;
    private final XmlGeneratorService xmlGeneratorService;
    private final ItbPostGenerationService itbPostGenerationService;
    
    public InitWorkflowService(
        GitService gitService,
        RobotExecutionService robotExecutionService,
        RobotReportService robotReportService,
        XmlGeneratorService xmlGeneratorService,
        ItbPostGenerationService itbPostGenerationService
    ) {
        this.gitService = gitService;
        this.robotExecutionService = robotExecutionService;
        this.robotReportService = robotReportService;
        this.xmlGeneratorService = xmlGeneratorService;
        this.itbPostGenerationService = itbPostGenerationService;
    }

    public void run(String repoUrl) throws Exception {

        Path workingDir = gitService.getWorkingDir();
        gitService.cloneRepository(repoUrl, workingDir);

        System.out.println("cloning step ok");

        RobotExecutionConfig config = new RobotExecutionConfig();
        config.setDryRun(true);
        config.setListener("listeners.SimpleListener.SimpleListener");
        config.setTargetPath(
            workingDir.resolve("TP").resolve("NGSI-LD")
        );
        config.setWorkingDirectory(workingDir);

        robotExecutionService.execute(config);

        System.out.println("dry-run step ok");

        Path reportPath = workingDir
                .getParent()
                .resolve("listeners/reports/simple.json");
        System.out.println("reportPath ok");

        RobotReport report = robotReportService.loadReport(reportPath);
        System.out.println("loadReport step ok");

        List<Scenario> scenarios = ScenarioFactory.fromRobotReport(report);
        System.out.println("scenarioFactory step ok");

        for (Scenario scenario : scenarios) {
            xmlGeneratorService.generateXmlTestCase(scenario);
        }
        System.out.println("generateXml step ok");

        itbPostGenerationService.completeAndDeployTestSuites();
        System.out.println("The JSON file and the test suite main file have been generated and copied in the ITB test suites");
    }
}
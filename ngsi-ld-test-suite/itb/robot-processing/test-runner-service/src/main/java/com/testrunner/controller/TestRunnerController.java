package com.testrunner.controller;

import com.testrunner.model.Request;
import com.testrunner.service.InitWorkflowService;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.http.*;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
public class TestRunnerController {

    private final InitWorkflowService initWorkflowService;
    private final String defaultRepoUrl;

    public TestRunnerController(InitWorkflowService initWorkflowService,
                               @Value("${testrunner.default-repo-url}") String defaultRepoUrl) {
        this.initWorkflowService = initWorkflowService;
        this.defaultRepoUrl = defaultRepoUrl;
    }

    @PostMapping("/init")
    public ResponseEntity<?> init(@RequestBody(required = false) Request request) {
        try {

            String repoUrl = (request == null || request.getRepoUrl() == null || request.getRepoUrl().isEmpty())
                    ? defaultRepoUrl
                    : request.getRepoUrl();

            initWorkflowService.run(repoUrl);

            return ResponseEntity.ok("Initialization done (repo : " + repoUrl + ")");

        } catch (Exception e) {
            return ResponseEntity.status(HttpStatus.INTERNAL_SERVER_ERROR)
                    .body("Error during the initialization : " + e.getMessage());
        }
    }
}
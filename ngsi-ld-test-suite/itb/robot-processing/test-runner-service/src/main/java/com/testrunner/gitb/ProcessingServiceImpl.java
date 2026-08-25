package com.testrunner.gitb;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.gitb.core.*;
import com.gitb.ps.Void;
import com.gitb.ps.*;
import com.gitb.tr.TestResultType;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Component;

import com.testrunner.service.GitService;
import com.testrunner.service.robot.*;


/**
 * Spring component that realises the processing service.
 */
@Component
public class ProcessingServiceImpl implements ProcessingService {

    private final GitService gitService;
    private final RobotExecutionService robotExecutionService;
    private final RobotListenerResultParser robotListenerResultParser;

    public ProcessingServiceImpl(
        GitService gitService,
        RobotExecutionService robotExecutionService,
        RobotListenerResultParser robotListenerResultParser
    ) {
        this.gitService = gitService;
        this.robotExecutionService = robotExecutionService;
        this.robotListenerResultParser = robotListenerResultParser;
    }

    /** Logger. */
    private static final Logger LOG = LoggerFactory.getLogger(ProcessingServiceImpl.class);

    @Autowired
    private Utils utils = null;

    /**
     * The purpose of the getModuleDefinition call is to inform its caller on how the service is supposed to be called.
     * <p/>
     * Note that defining the implementation of this service is optional, and can be empty unless you plan to publish
     * the service for use by third parties (in which case it serves as documentation on its expected inputs and outputs).
     *
     * @param parameters No parameters are expected.
     * @return The response.
     */
    @Override
    public GetModuleDefinitionResponse getModuleDefinition(Void parameters) {
        return new GetModuleDefinitionResponse();
    }

    /**
     * The purpose of the process operation is to execute one of the service's supported operations.
     * <p/>
     * What would typically take place here is as follows:
     * <ol>
     *    <li>Check that the requested operation is indeed supported by the service.</li>
     *    <li>For the requested operation collect and check the provided input parameters.</li>
     *    <li>Perform the requested operation and return the result to the test bed.</li>
     * </ol>
     *
     * @param processRequest The requested operation and input parameters.
     * @return The result.
     */
    @Override
    public ProcessResponse process(ProcessRequest processRequest) {
        LOG.info("Received 'process' command from test bed for session [{}]", processRequest.getSessionId());

        ProcessResponse response = new ProcessResponse();
        response.setReport(utils.createReport(TestResultType.SUCCESS));

        String jsonBody = utils.getRequiredString(processRequest.getInput(),"parameters");

        ObjectMapper objectMapper = new ObjectMapper();
        JsonNode root;

        try {

            root = objectMapper.readTree(jsonBody);

        } catch (Exception e) {
            throw new RuntimeException("Failed to parse JSON input", e);
        }

        String testName = root.path("testName").asText(null);
        if (testName == null || testName.isBlank()) {
            throw new IllegalArgumentException("testName is mandatory");
        }

        JsonNode parametersNode = root.path("parameters");
        if (!parametersNode.isObject()) {
            throw new IllegalArgumentException("parameters must be an object");
        }

        Path workingDir = gitService.getWorkingDir();

        RobotExecutionConfig config = new RobotExecutionConfig();
        config.setListener("listeners.RuntimeListener.RuntimeListener");
        config.setTargetPath(
            workingDir.resolve("TP").resolve("NGSI-LD")
        );
        config.setWorkingDirectory(workingDir);
        config.setTestName(testName);
        parametersNode.fields().forEachRemaining(entry -> {
            config.addVariable(entry.getKey(), entry.getValue().asText());
        });

        try {
            robotExecutionService.execute(config);
        } catch (IOException | InterruptedException e) {

            throw new RuntimeException(
                "Failed to execute the robot test",
                e
            );
        }

        System.out.println("robot execution step ok");

        Path reportPath = gitService
                .getWorkingDir()
                .getParent()
                .resolve("listeners/reports/runtime.json");

        // RobotReport report = robotReportService.loadReport(reportPath);
        String listenerJson;
        try {
            listenerJson = Files.readString(reportPath);
        } catch (IOException e) {
            throw new RuntimeException(
                "Failed to read listener JSON file", e
            );
        }
        System.out.println("loadReport step ok");

        AnyContent output = robotListenerResultParser.parseToAnyContent(listenerJson);

        LOG.info("Output :\n\n [{}].", output.getName());
        response.getOutput().add(output);
        return response;
    }

    /**
     * The purpose of the beginTransaction operation is to begin a unique processing session.
     * <p/>
     * Transactions are used when processing services need to maintain state across several calls. If this is needed
     * then this implementation would generate a session identifier and record the session for subsequent 'process' calls.
     * <p/>
     * In the typical case where no state needs to be maintained, you can provide an empty implementation for this method.
     *
     * @param beginTransactionRequest Optional configuration parameters to consider when starting a processing transaction.
     * @return The response with the generated session ID for the processing transaction.
     */
    @Override
    public BeginTransactionResponse beginTransaction(BeginTransactionRequest beginTransactionRequest) {
        return new BeginTransactionResponse();
    }

    /**
     * The purpose of the endTransaction operation is to complete an ongoing processing session.
     * <p/>
     * The main actions to be taken as part of this operation are to remove the provided session identifier (if this
     * was being recorded to begin with), and to perform any custom cleanup tasks.
     *
     * @param parameters The identifier of the session to terminate.
     * @return A void response.
     */
    @Override
    public Void endTransaction(BasicRequest parameters) {
        return new Void();
    }

}

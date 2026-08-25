*** Settings ***
Documentation       Check that one can query context source registrations. If present, the temporal query is matched against the observationInterval or the managementInterval

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Created Context Source Registrations
Test Template       Query Context Source Registration Matching Temporal Query


*** Variables ***
${context_source_registration_observation_interval_payload_file_path}=      csourceRegistrations/context-source-registration-observationInterval.jsonld
${context_source_registration_management_interval_payload_file_path}=       csourceRegistrations/context-source-registration-managementInterval.jsonld
${observation_interval_expectation_file_path}=                              csourceRegistrations/expectations/context-source-registrations-037-09-01.json
${management_interval_expectation_file_path}=                               csourceRegistrations/expectations/context-source-registrations-037-09-02.json


*** Test Cases ***    PAYLOAD_FILE_PATH    TIMEPROPERTY    EXPECTATION_FILE_PATH
037_09_01 Observation Interval With observedAt
    [Tags]    csr-query    5_10_2
    ${context_source_registration_observation_interval_payload_file_path}    observedAt    ${observation_interval_expectation_file_path}
037_09_02 Observation Interval Without Timeproperty
    [Tags]    csr-query    5_10_2
    ${context_source_registration_observation_interval_payload_file_path}    ${EMPTY}    ${observation_interval_expectation_file_path}
037_09_03 Management Interval With createdAt
    [Tags]    csr-query    5_10_2
    ${context_source_registration_management_interval_payload_file_path}    createdAt    ${management_interval_expectation_file_path}
037_09_04 Management Interval With modifiedAt
    [Tags]    csr-query    5_10_2
    ${context_source_registration_management_interval_payload_file_path}    modifiedAt    ${management_interval_expectation_file_path}


*** Keywords ***
Query Context Source Registration Matching Temporal Query
    [Documentation]    Check that one can query context source registrations. If present, the temporal query is matched against the observationInterval or the managementInterval
    [Arguments]    ${payload_file_path}    ${timeproperty}    ${expectation_file_path}

    Setup Initial Context Source Registrations    ${payload_file_path}

    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    type=Building
    ...    timeproperty=${timeproperty}
    ...    timerel=before
    ...    timeAt=2021-08-01T22:00:00Z

    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Context Source Registrations elements
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registration_ids}
    ...    ${response.json()}

Setup Initial Context Source Registrations
    [Arguments]    ${payload_file_path}
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${payload_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${context_source_registration_id}

Delete Created Context Source Registrations
    Delete Context Source Registration    ${context_source_registration_id}

*** Settings ***
Documentation       Check that one can query context source registrations matching property and relationship names of RegistrationInfo

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration
Test Teardown       Delete Created Context Source Registration
Test Template       Query Context Source Registration Matching Properties And Relationships Of RegistrationInfo


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-detailed-information.jsonld


*** Test Cases ***    ATTRS_VALUE    EXPECTATION_FILE_PATH
037_06_01 Query With Matching Properties And Relationships
    [Tags]    csr-query    5_10_2
    name,locatedAt    csourceRegistrations/expectations/context-source-registrations-037-06.json
037_06_02 Query Without Properties And Relationships
    [Tags]    csr-query    5_10_2
    ${EMPTY}    csourceRegistrations/expectations/context-source-registrations-037-06.json


*** Keywords ***
Query Context Source Registration Matching Properties And Relationships Of RegistrationInfo
    [Documentation]    Check that one can query context source registrations matching property and relationship names of RegistrationInfo
    [Arguments]    ${attrs_value}    ${expectation_file_path}
    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    type=Building
    ...    attrs=${attrs_value}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Context Source Registrations elements
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registration_ids}
    ...    ${response.json()}

Setup Initial Context Source Registration
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${context_source_registration_id}

Delete Created Context Source Registration
    Delete Context Source Registration    ${context_source_registration_id}

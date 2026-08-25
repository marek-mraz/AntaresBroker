*** Settings ***
Documentation       Check that one can retrieve a Context Source Registration

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Context Source Registration
Suite Teardown      Delete Created Context Source Registration


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration.jsonld
${expectation_file_path}=                               csourceRegistrations/expectations/context-source-registration.json


*** Test Cases ***
036_04_01 Retrieve Context Source Registration
    [Documentation]    Check that one can retrieve a Context Source Registration
    [Tags]    csr-retrieve    5_10_1
    ${response}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${context_source_registration_id}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Context Source Registration element
    ...    ${expectation_file_path}
    ...    ${context_source_registration_id}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Context Source Registration
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${context_source_registration_id}

Delete Created Context Source Registration
    Delete Context Source Registration    ${context_source_registration_id}

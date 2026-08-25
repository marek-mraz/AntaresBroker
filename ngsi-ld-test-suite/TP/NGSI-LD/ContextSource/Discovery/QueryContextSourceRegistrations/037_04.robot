*** Settings ***
Documentation       Check that one can query context source registrations. If a JSON-LD context is not provided, then all the query terms shall be resolved against the default JSON-LD @context

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Context Source Registration
Suite Teardown      Delete Created Context Source Registration


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-detailed-information.jsonld
${expectation_file_path}=                               csourceRegistrations/expectations/context-source-registrations-037-04.json
${entity_type}=                                         https://ngsi-ld-test-suite/context#Building


*** Test Cases ***
037_04_01 Query Context Source Registrations Without Context
    [Documentation]    Check that one can query context source registrations. If a JSON-LD context is not provided, then all the query terms shall be resolved against the default JSON-LD @context
    [Tags]    csr-query    5_10_2
    ${response}=    Query Context Source Registrations    id=${context_source_registration_id}    type=${entity_type}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Context Source Registrations elements
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registration_ids}
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

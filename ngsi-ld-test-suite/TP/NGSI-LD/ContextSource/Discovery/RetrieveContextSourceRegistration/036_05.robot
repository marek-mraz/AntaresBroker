*** Settings ***
Documentation       Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD @context is used if not present

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration
Test Teardown       Delete Created Context Source Registration
Test Template       Review JSON-LD resolution when retrieving a context source registration


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration.jsonld
${expectation_file_path_compacted}=                     csourceRegistrations/expectations/context-source-registration.json
${expectation_file_path_expanded}=                      csourceRegistrations/expectations/context-source-registration-expanded-format.json


*** Test Cases ***    CONTEXT    EXPECTED_PAYLOAD
036_05_01 EmptyJsonLdContext
    [Tags]    csr-retrieve    6_3_5
    ${EMPTY}    ${expectation_file_path_expanded}
036_05_02 CreationTimeJsonLdContext
    [Tags]    csr-retrieve    6_3_5
    ${ngsild_test_suite_context}    ${expectation_file_path_compacted}


*** Keywords ***
Review JSON-LD resolution when retrieving a context source registration
    [Documentation]    Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD @context is used if not present
    [Arguments]    ${context}    ${expected_payload}
    ${response}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${context_source_registration_id}
    ...    context=${context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Context Source Registration element
    ...    ${expected_payload}
    ...    ${context_source_registration_id}
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

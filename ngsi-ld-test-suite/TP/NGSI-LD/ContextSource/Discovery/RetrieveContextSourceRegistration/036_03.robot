*** Settings ***
Documentation       Check that one can retrieve a Context Source Registration. Term to URI expansion of Attribute names shall be observed.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Context Source Registration
Suite Teardown      Delete Created Context Source Registration


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration.jsonld
${expectation_file_path}=                               csourceRegistrations/expectations/context-source-registration-expanded-format.json


*** Test Cases ***
036_03_01 Retrieve Context Source Registration With Default Core Context
    [Documentation]    Check that one can retrieve a Context Source Registration. Term to URI expansion of Attribute names shall be observed.
    [Tags]    csr-retrieve    5_10_1
    ${response}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${context_source_registration_id}
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

*** Settings ***
Documentation       Check that one can query context source registrations matching EntityInfo of RegistrationInfo

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Created Context Source Registration
Test Template       Query Context Source Registration Matching EntityInfo of RegistrationInfo


*** Test Cases ***    REGISTRATION_FILE_PATH    EXPECTATION_FILE_PATH
037_05_01 Registration With EntityInfo Matching The Query
    [Tags]    csr-query    5_10_2
    csourceRegistrations/context-source-registration.jsonld    csourceRegistrations/expectations/context-source-registrations-037-05-01.json
037_05_02 Registration Without EntityInfo
    [Tags]    csr-query    5_10_2
    csourceRegistrations/context-source-registration-with-only-properties-information.jsonld    csourceRegistrations/expectations/context-source-registrations-037-05-02.json


*** Keywords ***
Query Context Source Registration Matching EntityInfo of RegistrationInfo
    [Documentation]    Check that one can query context source registrations matching EntityInfo of RegistrationInfo
    [Arguments]    ${registration_file_path}    ${expectation_file_path}
    Setup Initial Context Source Registrations    ${registration_file_path}

    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    type=OffStreetParking
    ...    idPattern=.*downtown$

    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Context Source Registrations elements
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registration_ids}
    ...    ${response.json()}

Setup Initial Context Source Registrations
    [Arguments]    ${registration_file_path}
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${registration_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${context_source_registration_id}

Delete Created Context Source Registration
    Delete Context Source Registration    ${context_source_registration_id}

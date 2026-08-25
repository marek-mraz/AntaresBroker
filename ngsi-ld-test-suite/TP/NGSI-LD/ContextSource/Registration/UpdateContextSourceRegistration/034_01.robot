*** Settings ***
Documentation       Check that one can update a context source registration by id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initialize the Test Case
Test Teardown       Delete Updated Context Source Registration
Test Template       Update A Context Source


*** Test Cases ***    FILENAME    UPDATE_FILENAME
034_01_01 Update A Context Source Registration By Id
    [Tags]    csr-update    5_9_3
    context-source-registration.jsonld    context-source-registration-update.json


*** Keywords ***
Update A Context Source
    [Documentation]    Check that one can update a context source registration by id
    [Arguments]    ${filename}    ${update_filename}
    Set Test Variable    ${filename}
    ${fragment}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/fragments/${update_filename}
    ${registration_update_fragment}=    Update Value To JSON    ${fragment}    $.id    ${registration_id}
    ${response}=    Update Context Source Registration With Return
    ...    ${registration_id}
    ...    ${registration_update_fragment}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${registration_id}
    Check JSON Value In Response Body
    ...    ['endpoint']
    ...    http://my.new.csource.org:1026
    ...    ${response1.json()}

Delete Updated Context Source Registration
    Delete Context Source Registration    ${registration_id}

Initialize the Test Case
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${payload}=    Load JSON From File
    ...    ${EXECDIR}/data/csourceRegistrations/context-source-registration.jsonld
    ${registration_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    Set Test Variable    ${registration_payload}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

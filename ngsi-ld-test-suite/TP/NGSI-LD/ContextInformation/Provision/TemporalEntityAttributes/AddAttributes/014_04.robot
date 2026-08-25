*** Settings ***
Documentation       Check that an error is raised if one adds a temporal entity attribute with empty/invalid content

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initialize Test Case
Test Teardown       Delete Temporal Entity
Test Template       Add an Attribute To a Temporal Entity From File


*** Variables ***
${filename}=                vehicle-temporal-representation.jsonld
${fragment_filename}=       vehicle-temporal-representation-fragment.jsonld
${status_code}=             400


*** Test Cases ***    UPDATE_FILENAME
014_04_01 Add An Attribute To A Temporal Representation Of An Entity With Invalid Content
    vehicle-temporal-representation-invalid-json-fragment.jsonld
014_04_02 Add An Attribute To A Temporal Representation Of An Entity With Empty Content
    vehicle-temporal-representation-empty-json-fragment.jsonld


*** Keywords ***
Add an Attribute To a Temporal Entity From File
    [Documentation]    Check that an error is raised if one adds a temporal entity attribute with empty/invalid content
    [Tags]    tea-append    5_6_12
    [Arguments]    ${update_filename}
    ${response}=    Append Attribute To Temporal Entity
    ...    ${temporal_entity_representation_id}
    ...    ${update_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Initialize Test Case
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

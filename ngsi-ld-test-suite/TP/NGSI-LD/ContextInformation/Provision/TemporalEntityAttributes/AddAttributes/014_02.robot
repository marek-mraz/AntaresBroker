*** Settings ***
Documentation       Check that an error is raised if one adds an attribute to a temporal entity with invalid content

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Temporal Entity
Test Template       Add Attribute To Temporal Entity


*** Variables ***
${filename}=                vehicle-temporal-representation.jsonld
${fragment_filename}=       vehicle-temporal-representation-fragment.jsonld
${status_code}=             400


*** Test Cases ***    ID
014_02_01 Add An Attribute To A Temporal Representation Of An Entity With An Empty Entity Id
    ${EMPTY}
014_02_02 Add An Attribute To A Temporal Representation Of An Entity With An Invalid Entity Id
    thisIsAninvalidId


*** Keywords ***
Add Attribute To Temporal Entity
    [Documentation]    Check that an error is raised if one adds a temporal entity attribute with a non-existing/invalid EntityId
    [Tags]    tea-append    5_6_12
    [Arguments]    ${id}
    ${response}=    Append Attribute To Temporal Entity    ${id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}
    ${create_response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

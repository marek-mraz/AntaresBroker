*** Settings ***
Documentation       Check that one can create a temporal representation of an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Temporal Entity
Test Template       Create Temporal Entity


*** Test Cases ***    FILENAME    EXPECTATION_FILENAME    CONTENT_TYPE
007_01_01 Create A Temporal Representation Of An Entity
    vehicle-create-temporal-representation.jsonld    vehicle-temporal-representation-create.jsonld    application/ld+json
007_01_02 Create A Temporal Entity With No Context
    vehicle-create-temporal-representation-without-context.jsonld    vehicle-temporal-representation-create-with-no-context.jsonld    application/json


*** Keywords ***
Create Temporal Entity
    [Documentation]    Check that one can create a temporal representation of an entity
    [Tags]    te-create    5_6_11
    [Arguments]    ${filename}    ${expectation_filename}    ${content_type}
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}

    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${content_type}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${created_temporal_entity}=    Load Test Sample
    ...    temporalEntities/${filename}
    ...    ${temporal_entity_representation_id}
    IF    '${content_type}'=='application/json'
        ${response1}=    Retrieve Temporal Representation Of Entity
        ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    END
    IF    '${content_type}'=='application/ld+json'
        ${response1}=    Retrieve Temporal Representation Of Entity
        ...    temporal_entity_representation_id=${temporal_entity_representation_id}
        ...    context=${ngsild_test_suite_context}
    END
    ${ignored_attributes}=    Create List    instanceId    @context
    ${temporal_entity_expectation_payload}=    Load Test Sample
    ...    temporalEntities/expectations/${expectation_filename}
    ...    ${temporal_entity_representation_id}
    Check Created Resource Set To
    ...    ${temporal_entity_expectation_payload}
    ...    ${response1.json()}
    ...    ${ignored_attributes}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

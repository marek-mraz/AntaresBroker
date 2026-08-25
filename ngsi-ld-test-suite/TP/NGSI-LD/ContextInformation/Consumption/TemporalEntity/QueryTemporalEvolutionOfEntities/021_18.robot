*** Settings ***
Documentation       Check that one can filter attribute instances of a temporal entity based on datasetId.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Temporal Entity
Suite Teardown      Delete Temporal Entity
Test Template       Filter attribute instances of a temporal entity based on datasetId


*** Variables ***
${filename}     vehicle-temporal-representation-with-multi-attributes-instances.jsonld
${timerel}      after
${timeat}       2020-08-01T12:04:00Z


*** Test Cases ***    ATTRS    DATASET_ID    EXPECTATION_FILENAME
021_18_01 Filter Based On Attrs Only
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    speed    ${EMPTY}    vehicle-temporal-representation-speed-attribute.json
021_18_02 Filter Based On datasetId Only
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    ${EMPTY}    urn:ngsi-ld:Dataset:Common    vehicle-temporal-representation-common-datasetid.json
021_18_03 Filter Based On Two datasetIds
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    ${EMPTY}    urn:ngsi-ld:Dataset:Common,urn:ngsi-ld:Dataset:Speed    vehicle-temporal-representation-two-datasetids.json
021_18_04 Filter Based On Default Instance
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    ${EMPTY}    @none    vehicle-temporal-representation-default-instances.json
021_18_05 Filter Based On Attrs And Default Instance
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    speed    @none    vehicle-temporal-representation-speed-default-instance.json
021_18_06 Filter Based On Attrs And datasetId
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    fuelLevel    urn:ngsi-ld:Dataset:fuel    vehicle-temporal-representation-fuellevel-attribute-fuel-datasetid.json
021_18_07 Filter Based On Attrs And datasetId With No Match
    [Tags]    te-query    4_5_5    5_7_4    since_v1.8.1
    speed    urn:ngsi-ld:Dataset:fuel    empty.json


*** Keywords ***
Filter attribute instances of a temporal entity based on datasetId
    [Documentation]    Check that one can filter attribute instances of a temporal entity based on datasetId.
    [Arguments]    ${attrs}    ${datasetId}    ${expectation_filename}

    ${attrsToMatch}=    Catenate
    ...    SEPARATOR=,
    ...    ${attrs}

    ${response}=    Query Temporal Representation Of Entities
    ...    attrs=${attrsToMatch}
    ...    datasetId=${datasetId}
    ...    entity_types=Vehicle
    ...    timerel=${timerel}
    ...    timeAt=${timeat}
    ...    context=${ngsild_test_suite_context}
    ...    options=temporalValues

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    filename=${expectation_filename}
    ...    temporal_entity_representation_id=${temporal_entity_id}
    ...    response_body=${response.json()}

Setup Initial Temporal Entity
    ${temporal_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-18-A
    Set Suite Variable    ${temporal_entity_id}
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${filename}
    ...    ${temporal_entity_id}
    Check Response Status Code    201    ${create_response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_id}
